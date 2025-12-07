use std::{env, error::Error, fmt, fs};

use dotenvy::dotenv;
use futures::StreamExt;
use mongodb::{
    Client,
    bson::{DateTime, Document, doc, to_document},
    options::UpdateOptions,
};
use reqwest::header::{ACCEPT, HeaderValue, USER_AGENT};
use serde_json::{Value, json};
use xai_rs::{
    AsyncClient, XaiError,
    client::async_client::xai_api::tool_call,
    models::{ChatMessage, GetCompletionsRequestExt, ToolDefinition, build_request},
};

/// Endpoint + query params we call on the X API.
const NEWS_ENDPOINT: &str = "https://api.x.com/2/news/1989418137272422538";
const NEWS_FIELDS: &str = "contexts,cluster_posts_results";

/// Required/optional environment variables.
const BEARER_ENV: &str = "BEARER";
const XAI_KEY_ENV: &str = "XAI_API_KEY";
const XAI_MODEL_ENV: &str = "XAI_MODEL"; // optional, defaults below
const MONGO_URI_ENV: &str = "MONGO_URI"; // optional, controls Mongo upsert
const MONGO_DB_ENV: &str = "MONGO_DB"; // optional, defaults below
const MONGO_COLLECTION_ENV: &str = "MONGO_COLLECTION"; // optional, defaults below
const DEFAULT_MONGO_DB: &str = "grokipedia";
const DEFAULT_MONGO_COLLECTION: &str = "summaries";
const SUMMARY_OUTPUT_PATH: &str = "summary.json";

const APP_USER_AGENT: &str = "grokipedia-x/0.1";
const DEFAULT_XAI_MODEL: &str = "grok-4-fast-non-reasoning";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenv().ok();
    let bearer_token = env::var(BEARER_ENV).map_err(|_| {
        format!("Missing {BEARER_ENV} env var. Export a valid X API bearer token before running.")
    })?;
    let xai_api_key = env::var(XAI_KEY_ENV).map_err(|_| {
        format!("Missing {XAI_KEY_ENV} env var. Export a valid xAI API key before running.")
    })?;
    let xai_model = env::var(XAI_MODEL_ENV).unwrap_or_else(|_| DEFAULT_XAI_MODEL.to_string());

    let news_payload = fetch_news(&bearer_token).await?;
    let announcements_payload = fetch_announcements(&bearer_token).await?;
    println!(
        "=== Raw X News payload ===\n{}",
        serde_json::to_string_pretty(&news_payload)?
    );
    println!(
        "=== Raw X Announcements payload ===\n{}",
        serde_json::to_string_pretty(&announcements_payload)?
    );

    // CHANGE THE SOURCE OF INFORMATION HERE
    let summary = summarize_with_xai(&announcements_payload, &xai_api_key, &xai_model).await?;
    println!("\n=== xAI summary ({xai_model}) ===\n{summary}");

    let summary_payload = json!({
        "model": xai_model,
        "summary": summary,
    });
    fs::write(
        SUMMARY_OUTPUT_PATH,
        serde_json::to_string_pretty(&summary_payload)?,
    )?;
    println!("Saved summary to {SUMMARY_OUTPUT_PATH}");

    if let Err(err) = upsert_summary_in_mongo(&summary_payload).await {
        eprintln!("Failed to upsert summary in MongoDB: {err}");
    }

    Ok(())
}

async fn upsert_summary_in_mongo(summary_payload: &Value) -> Result<(), Box<dyn Error>> {
    let mongo_uri = match env::var(MONGO_URI_ENV) {
        Ok(uri) => uri,
        Err(_) => return Ok(()),
    };

    let db_name = env::var(MONGO_DB_ENV).unwrap_or_else(|_| DEFAULT_MONGO_DB.to_string());
    let collection_name =
        env::var(MONGO_COLLECTION_ENV).unwrap_or_else(|_| DEFAULT_MONGO_COLLECTION.to_string());

    let client = Client::with_uri_str(&mongo_uri).await?;
    let collection = client
        .database(&db_name)
        .collection::<Document>(&collection_name);

    let doc_id = summary_payload
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or("latest-summary")
        .to_string();

    let mut document = to_document(summary_payload)?;
    document.insert("_id", doc_id.clone());
    document.insert("updated_at", DateTime::now());

    let filter = doc! { "_id": &doc_id };
    let update = doc! { "$set": document };
    let options = UpdateOptions::builder().upsert(true).build();

    collection.update_one(filter, update, options).await?;
    println!(
        "Upserted summary into MongoDB collection \"{}\" (db: \"{}\", id: \"{}\")",
        collection_name, db_name, doc_id
    );

    Ok(())
}

async fn fetch_news(bearer_token: &str) -> Result<Value, Box<dyn Error>> {
    // let url = format!("{NEWS_ENDPOINT}?news.fields={NEWS_FIELDS}");
    let url = format!(
        "https://api.x.com/2/tweets/search/recent?max_results=50&query=breaking%20min_likes%3A10000%20min_reposts%3A1000%20is%3Averified%20-has%3Ahashtags%20lang%3Aen%20-has%3Alinks"
    );

    let client = reqwest::Client::new();

    let response = client
        .get(url)
        .bearer_auth(bearer_token)
        .header(ACCEPT, HeaderValue::from_static("application/json"))
        .header(USER_AGENT, HeaderValue::from_static(APP_USER_AGENT))
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json().await?)
}

async fn fetch_announcements(bearer_token: &str) -> Result<Value, Box<dyn Error>> {
    let url = format!(
        "https://api.x.com/2/tweets/search/recent?max_results=50&query=announcement%20min_likes%3A100%20min_reposts%3A100%20is%3Averified%20-has%3Ahashtags%20lang%3Aen%20has%3Alinks"
    );
    let client = reqwest::Client::new();

    let response = client
        .get(url)
        .bearer_auth(bearer_token)
        .header(ACCEPT, HeaderValue::from_static("application/json"))
        .header(USER_AGENT, HeaderValue::from_static(APP_USER_AGENT))
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json().await?)
}

async fn summarize_with_xai(
    news_payload: &Value,

    api_key: &str,
    model: &str,
) -> Result<String, Box<dyn Error>> {
    let mut client = AsyncClient::new(api_key.to_owned())
        .await
        .map_err(map_client_error)?;

    let user_prompt = format!(
        "Here is the JSON payload returned by the X News endpoint: {}. You are the real time pipeline agent for breaking X news to Grokipedia. Based on the entire JSON, the breaking news data provided, determine what are the concerning organizations or individuals involved and find an existing Grokipedia article that matches the context of that same organization or individual. Here is what I need in from you: the url of the grokipedia page (if it exists), then i need your suggested edit based on that initial JSON payload of relevant news, and finally i need you to grab the ORIGINAL TEXT within that grokipedia page that is subject to be changed and updated. Use your tools to go to the URL of the grokipedia page if it exists in order to fetch REAL text from the article that is the MOST relevant to the suggested edit from the news claim. Word for word, that will be the ORIGINAL TEXT. Make sure the JSON has inner objects of multiple entries for this which each have the fields that i requested.",
        serde_json::to_string(news_payload)?
    );

    let request = build_request(
        vec![
            ChatMessage::system(
                "You are the real time pipeline agent for breaking X news to Grokipedia. Based on the breaking news data provided, determine what are the concerning organizations or individuals involved and find an existing Grokipedia article that matches the context of that same organization or individual.",
            ),
            ChatMessage::user(user_prompt),
        ],
        model,
    )
    .with_tools(vec![
        ToolDefinition::web_search(),
        ToolDefinition::x_search(),
        ToolDefinition::code_execution(),
    ])
    .with_parallel_tool_calls(true);

    let mut stream = client
        .get_completion_chunk(request)
        .await
        .map_err(map_client_error)?
        .into_inner();

    let mut summary = String::new();

    while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result.map_err(|status| map_status_message(status.to_string()))?;

        for output in chunk.outputs {
            if let Some(delta) = output.delta {
                for tool_call in delta.tool_calls {
                    println!("Running {:?} tool...", tool_call.r#type());
                    if let Some(tool_call::Tool::Function(function)) = tool_call.tool {
                        println!(
                            "Calling tool: {} with arguments: {}",
                            function.name, function.arguments
                        );
                    }
                }

                if !delta.content.is_empty() {
                    summary.push_str(&delta.content);
                }
            }
        }
    }

    if summary.trim().is_empty() {
        Err("xAI response missing completion text".into())
    } else {
        Ok(summary)
    }
}

fn map_client_error(err: XaiError) -> Box<dyn Error> {
    let msg = err.to_string();
    if msg.contains("invalid compression flag") && msg.contains("504") {
        Box::new(AgenticError(format!(
            "xAI agentic stream aborted (gateway timeout before first chunk). Raw error: {msg}"
        )))
    } else {
        Box::new(err)
    }
}

fn map_status_message(msg: String) -> Box<dyn Error> {
    if msg.contains("invalid compression flag") && msg.contains("504") {
        Box::new(AgenticError(format!(
            "xAI agentic stream aborted (gateway timeout before first chunk). Raw error: {msg}"
        )))
    } else {
        Box::new(AgenticError(msg))
    }
}

#[derive(Debug)]
struct AgenticError(String);

impl fmt::Display for AgenticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for AgenticError {}
