use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

use crate::blockchain_bridge::AssetListingView;

#[derive(Debug, Serialize, Deserialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    system: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaResponse {
    response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchResult {
    pub listing_id: u64,
    pub seller: String,
    pub description: String,
    pub price_avax: String,
    pub price_wei: String,
    pub token_id: u64,
    pub reason: String,
}

/// Matches a buyer's free-text intent against real on-chain listings using
/// the same local Ollama model the Shark negotiation agent already uses.
pub struct MatchAgent {
    client: Client,
    ollama_url: String,
}

impl MatchAgent {
    pub fn new(ollama_url: Option<String>) -> Self {
        MatchAgent {
            client: Client::new(),
            ollama_url: ollama_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
        }
    }

    pub async fn match_intent(
        &self,
        intent: &str,
        price_ceiling: f64,
        listings: &[AssetListingView],
    ) -> Result<Option<MatchResult>, Box<dyn Error>> {
        if listings.is_empty() {
            return Ok(None);
        }

        let catalog = listings
            .iter()
            .map(|l| format!("- id {}: \"{}\" — {} AVAX", l.id, l.description, l.price_avax))
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            r#"You match a buyer's intent to the single best listing from a marketplace catalog.

RULES:
1. Only choose a listing if it genuinely matches what the buyer is looking for.
2. If nothing matches well, return matched_id: null.
3. Never reason about price limits yourself — that is checked separately.
4. Respond ONLY with JSON in this exact format:
{{
  "matched_id": <listing id number or null>,
  "reason": "<brief explanation>"
}}

Catalog:
{}
"#,
            catalog
        );

        let request = OllamaRequest {
            model: "llama2".to_string(),
            prompt: format!("Buyer intent: {}", intent),
            stream: false,
            system: Some(system_prompt),
        };

        let response = self
            .client
            .post(format!("{}/api/generate", self.ollama_url))
            .json(&request)
            .send()
            .await?;

        let ollama_response: OllamaResponse = response.json().await?;
        let json_slice = crate::llm_json::extract_json_object(&ollama_response.response);

        let parsed: serde_json::Value = serde_json::from_str(json_slice)
            .unwrap_or_else(|_| serde_json::json!({ "matched_id": null, "reason": "No match" }));

        // The model frequently emits JSON-shaped-but-invalid output (e.g.
        // `"matched_id": id 3` instead of `3`), which fails strict parsing
        // above and silently looks like "no match" even though the model
        // did pick something — recover the id with a looser scan before
        // giving up.
        let matched_id = match parsed["matched_id"].as_u64() {
            Some(id) => id,
            None => match crate::llm_json::recover_number_field(json_slice, "matched_id") {
                Some(id) => id,
                None => return Ok(None),
            },
        };

        let listing = match listings.iter().find(|l| l.id == matched_id) {
            Some(l) => l,
            None => return Ok(None), // model hallucinated an id that doesn't exist
        };

        // Never trust the model on money — verify the price ceiling ourselves.
        let price: f64 = listing.price_avax.parse().unwrap_or(f64::MAX);
        if price > price_ceiling {
            println!(
                "⚠️  Match rejected: listing {} price {} AVAX exceeds ceiling {} AVAX",
                matched_id, price, price_ceiling
            );
            return Ok(None);
        }

        Ok(Some(MatchResult {
            listing_id: listing.id,
            seller: listing.seller.clone(),
            description: listing.description.clone(),
            price_avax: listing.price_avax.clone(),
            price_wei: listing.price_wei.clone(),
            token_id: listing.token_id,
            reason: parsed["reason"].as_str().unwrap_or("Matched").to_string(),
        }))
    }
}
