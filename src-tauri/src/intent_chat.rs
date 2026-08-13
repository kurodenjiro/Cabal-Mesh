//! Parses free text into `IntentFields` via the configured LLM.
//!
//! # The safety property this preserves
//!
//! This module produces exactly the same [`IntentFields`] shape the
//! hand-filled form already sends to `parse_draft` — never a validated
//! `IntentDraft`, never anything closer to broadcast than that. Whatever the
//! model returns is exactly as trusted as what a user typed into a raw text
//! box, no more: `preview_intent` and `broadcast_intent` validate it
//! identically either way, and neither this module nor the command wrapping
//! it can skip that step. See `docs/intent-chat-and-modules-design.md`.

use crate::commands::{FormOptions, IntentFields};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    system: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
}

/// The model's answer, every field optional on the wire — free text
/// reasonably leaves fields unmentioned ("buy 10 avax" says nothing about
/// privacy), and a blank here becomes `parse_draft`'s ordinary missing-field
/// rejection later, the same path an unfilled form field already takes.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ParsedFields {
    action: String,
    asset: String,
    condition: String,
    price: String,
    amount: String,
    mode: String,
    privacy: String,
}

impl From<ParsedFields> for IntentFields {
    fn from(parsed: ParsedFields) -> Self {
        Self {
            action: parsed.action,
            asset: parsed.asset,
            condition: parsed.condition,
            price: parsed.price,
            amount: parsed.amount,
            mode: parsed.mode,
            privacy: parsed.privacy,
        }
    }
}

/// Builds the instruction that tells the model exactly which strings are
/// legal for each field — the same vocabulary the segmented controls offer
/// (`intent_form_options`), embedded here rather than duplicated, so a
/// second hardcoded list can never drift from what the form actually
/// accepts.
fn system_prompt(options: &FormOptions) -> String {
    let actions = options.actions.join(", ");
    let assets = options.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ");
    let conditions = options.conditions.iter().map(|c| format!("\"{c}\"")).collect::<Vec<_>>().join(", ");
    let modes = options.modes.iter().map(|m| format!("\"{}\"", m.label)).collect::<Vec<_>>().join(", ");
    let privacy = options.privacy_levels.join(", ");

    format!(
        r#"You convert a trading request written in plain English into structured JSON fields for the CabalMesh intent form. Extract only what the text actually states, and leave a field as an empty string "" when it is not mentioned. Never invent a number, a price, or a choice the text does not support.

Allowed values (copy one exactly, case-sensitive where quoted):
- action: one of {actions}
- asset: one of {assets}
- condition: one of {conditions}
- price: a plain number in US dollars, only when condition is not the "any price" option; otherwise ""
- amount: a plain number, no currency symbol or asset name
- mode: one of {modes}
- privacy: one of {privacy}

Respond with ONLY this JSON object, nothing else, no explanation:
{{"action": "", "asset": "", "condition": "", "price": "", "amount": "", "mode": "", "privacy": ""}}"#
    )
}

/// Turns free text into intent fields via the configured Ollama endpoint.
pub struct IntentChatParser {
    client: Client,
    ollama_url: Option<String>,
}

impl IntentChatParser {
    #[must_use]
    pub fn new(ollama_url: Option<String>) -> Self {
        Self { client: Client::new(), ollama_url }
    }

    /// Resolved per request, matching `SharkAgent::url` — a URL set at
    /// runtime (the only way to reach a model on iOS, which has no local
    /// process) applies without a restart.
    fn url(&self) -> String {
        self.ollama_url.clone().unwrap_or_else(crate::ollama_config::url)
    }

    /// Asks the model to turn `text` into intent fields, telling it exactly
    /// which values `options` makes legal.
    ///
    /// # Errors
    ///
    /// The request to Ollama failed outright (network, timeout, non-2xx). A
    /// response that isn't valid JSON is deliberately **not** an error here
    /// — it becomes every field blank, which the caller's own validation
    /// (`parse_draft`) rejects with the same per-field messages an empty
    /// form submission already gets, rather than a different failure mode
    /// this one path would need its own copy for.
    pub async fn parse(&self, text: &str, options: &FormOptions) -> Result<IntentFields, Box<dyn Error>> {
        let request = OllamaRequest {
            model: "llama2".to_string(),
            prompt: format!("Convert this request:\n\n{text}"),
            stream: false,
            system: Some(system_prompt(options)),
        };

        let response = self.client.post(format!("{}/api/generate", self.url())).json(&request).send().await?;
        let ollama_response: OllamaResponse = response.json().await?;

        let parsed: ParsedFields =
            serde_json::from_str(crate::llm_json::extract_json_object(&ollama_response.response)).unwrap_or_default();

        Ok(parsed.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{AssetOption, ModeOption};

    fn options() -> FormOptions {
        FormOptions {
            actions: vec!["BUY".into(), "SELL".into()],
            assets: vec![AssetOption { name: "AVAX".into(), tag: "AVX".into(), decimals: 18, available: None }],
            conditions: vec!["Price under".into(), "Any price".into()],
            modes: vec![ModeOption { label: "SHARK MODE".into(), description: String::new() }],
            privacy_levels: vec!["HIGH".into()],
        }
    }

    #[test]
    fn the_prompt_names_every_allowed_value_so_none_is_invented() {
        let prompt = system_prompt(&options());
        for expected in ["BUY", "SELL", "AVAX", "Price under", "Any price", "SHARK MODE", "HIGH"] {
            assert!(prompt.contains(expected), "prompt is missing {expected:?}: {prompt}");
        }
    }

    #[test]
    fn a_well_formed_response_becomes_intent_fields() {
        let raw = r#"{"action": "BUY", "asset": "AVAX", "condition": "Price under", "price": "95", "amount": "10", "mode": "SHARK MODE", "privacy": "HIGH"}"#;
        let parsed: ParsedFields = serde_json::from_str(raw).unwrap();
        let fields: IntentFields = parsed.into();
        assert_eq!(fields.action, "BUY");
        assert_eq!(fields.amount, "10");
        assert_eq!(fields.mode, "SHARK MODE");
    }

    #[test]
    fn a_response_wrapped_in_prose_still_parses() {
        // llama2's own habit, per llm_json.rs — reused here rather than
        // re-solved.
        let raw = "Sure! Here you go:\n{\"action\": \"BUY\", \"asset\": \"AVAX\", \"condition\": \"\", \"price\": \"\", \"amount\": \"10\", \"mode\": \"\", \"privacy\": \"\"}\nLet me know if you need anything else.";
        let parsed: ParsedFields = serde_json::from_str(crate::llm_json::extract_json_object(raw)).unwrap();
        assert_eq!(parsed.action, "BUY");
        assert_eq!(parsed.amount, "10");
    }

    #[test]
    fn a_field_the_text_never_mentioned_is_left_blank_not_invented() {
        let raw = r#"{"action": "BUY", "amount": "10"}"#;
        let parsed: ParsedFields = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.privacy, "", "an unmentioned field must stay blank, not default to a guess");
        assert_eq!(parsed.condition, "");
    }

    #[test]
    fn garbage_that_is_not_json_at_all_becomes_every_field_blank_not_a_panic() {
        let raw = "I'm not sure what you mean by that.";
        let parsed: ParsedFields = serde_json::from_str(crate::llm_json::extract_json_object(raw)).unwrap_or_default();
        let fields: IntentFields = parsed.into();
        assert_eq!(fields.action, "");
        assert_eq!(fields.amount, "");
    }
}
