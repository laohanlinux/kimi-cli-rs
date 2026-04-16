use secrecy::ExposeSecret;
use std::collections::HashSet;
use strum::{Display, EnumString, VariantNames};

pub use crate::config::ModelCapability;

/// Supported LLM provider types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, EnumString, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum ProviderType {
    Kimi,
    #[strum(serialize = "openai_legacy")]
    OpenAiLegacy,
    #[strum(serialize = "openai_responses")]
    OpenAiResponses,
    Anthropic,
    #[strum(serialize = "google_genai")]
    GoogleGenAi,
    Gemini,
    VertexAi,
    #[strum(serialize = "_echo")]
    Echo,
    #[strum(serialize = "_scripted_echo")]
    ScriptedEcho,
    #[strum(serialize = "_chaos")]
    Chaos,
}

/// All defined model capabilities.
pub const ALL_MODEL_CAPABILITIES: &[ModelCapability] = &[
    ModelCapability::ImageIn,
    ModelCapability::VideoIn,
    ModelCapability::Thinking,
    ModelCapability::AlwaysThinking,
];

/// LLM client wrapper.
#[derive(Debug, Clone)]
pub struct Llm {
    pub model_name: String,
    pub max_context_size: usize,
    pub capabilities: HashSet<ModelCapability>,
    pub thinking: Option<bool>,
    pub provider_type: ProviderType,
    pub base_url: String,
    pub api_key: secrecy::SecretString,
}

impl Default for Llm {
    fn default() -> Self {
        Self {
            model_name: String::new(),
            max_context_size: 128_000,
            capabilities: HashSet::new(),
            thinking: None,
            provider_type: ProviderType::Kimi,
            base_url: String::new(),
            api_key: secrecy::SecretString::new(String::new().into()),
        }
    }
}

/// OpenAI-compatible chat request payload.
#[derive(Debug, serde::Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDef>>,
}

/// OpenAI-compatible chat message.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ChatMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCallDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// OpenAI-compatible tool definition.
#[derive(Debug, serde::Serialize)]
struct ToolDef {
    #[serde(rename = "type")]
    tool_type: String,
    function: FunctionDef,
}

#[derive(Debug, serde::Serialize)]
struct FunctionDef {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    parameters: serde_json::Value,
}

/// OpenAI-compatible tool call in assistant message.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ToolCallDef {
    id: String,
    #[serde(rename = "type")]
    tool_type: String,
    function: FunctionCallDef,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct FunctionCallDef {
    name: String,
    arguments: String,
}

/// OpenAI-compatible chat response.
#[derive(Debug, serde::Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, serde::Deserialize)]
struct Choice {
    message: ChatMessage,
}

impl Llm {
    /// Sends a chat request to the provider and returns the assistant message.
    #[tracing::instrument(level = "debug", skip(self, system_prompt, history, tools))]
    pub async fn chat(
        &self,
        system_prompt: Option<&str>,
        history: &[crate::soul::message::Message],
        tools: Option<&crate::soul::toolset::KimiToolset>,
    ) -> crate::error::Result<crate::soul::message::Message> {
        if self.base_url.is_empty() {
            return Ok(crate::soul::message::Message {
                role: "assistant".into(),
                content: vec![crate::soul::message::ContentPart::Text {
                    text: "LLM base_url is not configured.".into(),
                }],
                tool_calls: None,
                tool_call_id: None,
            });
        }

        let mut messages = Vec::new();
        if let Some(prompt) = system_prompt {
            messages.push(ChatMessage {
                role: "system".into(),
                content: Some(prompt.into()),
                tool_calls: None,
                tool_call_id: None,
            });
        }
        for msg in history {
            messages.push(convert_message(msg));
        }

        let tool_defs = tools.map(|t| convert_tools(t));

        let request = ChatRequest {
            model: self.model_name.clone(),
            messages,
            tools: tool_defs,
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()?;

        let url = if self.base_url.ends_with('/') {
            format!("{}v1/chat/completions", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key.expose_secret()))
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::error::KimiCliError::Generic(
                format!("LLM request failed: HTTP {status} - {body}"),
            ));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| crate::error::KimiCliError::Generic(format!("Failed to parse LLM response: {e}")))?;

        let choice = chat_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| crate::error::KimiCliError::Generic("Empty LLM response choices".into()))?;

        Ok(convert_chat_message(&choice.message))
    }
}

fn convert_message(msg: &crate::soul::message::Message) -> ChatMessage {
    ChatMessage {
        role: msg.role.clone(),
        content: Some(msg.extract_text("")),
        tool_calls: msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| ToolCallDef {
                    id: c.id.clone(),
                    tool_type: "function".into(),
                    function: FunctionCallDef {
                        name: c.name.clone(),
                        arguments: c.arguments.to_string(),
                    },
                })
                .collect()
        }),
        tool_call_id: msg.tool_call_id.clone(),
    }
}

fn convert_chat_message(msg: &ChatMessage) -> crate::soul::message::Message {
    crate::soul::message::Message {
        role: msg.role.clone(),
        content: vec![crate::soul::message::ContentPart::Text {
            text: msg.content.clone().unwrap_or_default(),
        }],
        tool_calls: msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| crate::soul::message::ToolCall {
                    id: c.id.clone(),
                    name: c.function.name.clone(),
                    arguments: serde_json::from_str(&c.function.arguments).unwrap_or_else(|_| {
                        serde_json::json!({"raw": c.function.arguments.clone()})
                    }),
                })
                .collect()
        }),
        tool_call_id: msg.tool_call_id.clone(),
    }
}

fn convert_tools(toolset: &crate::soul::toolset::KimiToolset) -> Vec<ToolDef> {
    toolset
        .tools_sync()
        .iter()
        .map(|(name, tool)| ToolDef {
            tool_type: "function".into(),
            function: FunctionDef {
                name: name.clone(),
                description: Some(tool.description().into()),
                parameters: tool.parameters_schema(),
            },
        })
        .collect()
}

/// Returns a display-friendly model name.
pub fn model_display_name(model_name: Option<&str>) -> String {
    match model_name {
        Some("kimi-for-coding") | Some("kimi-code") => {
            format!("{} (powered by kimi-k2.5)", model_name.unwrap())
        }
        Some(name) => name.to_string(),
        None => String::new(),
    }
}

/// Creates an LLM instance from the given provider and model configuration.
#[tracing::instrument(level = "debug")]
pub async fn create_llm(
    provider: &crate::config::LlmProvider,
    model: &crate::config::LlmModel,
    thinking: Option<bool>,
    _session_id: Option<&str>,
) -> crate::error::Result<Option<Llm>> {
    let provider_type = provider
        .r#type
        .parse()
        .unwrap_or(ProviderType::Kimi);
    let mut capabilities = derive_model_capabilities(model);
    if thinking.unwrap_or(false) {
        capabilities.insert(ModelCapability::Thinking);
    }
    Ok(Some(Llm {
        model_name: model.model.clone(),
        max_context_size: model.max_context_size,
        capabilities,
        thinking,
        provider_type,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
    }))
}

/// Clones an existing LLM, optionally switching to a different model alias.
#[tracing::instrument(level = "debug")]
pub async fn clone_llm_with_model_alias(
    llm: Option<&Llm>,
    config: &crate::config::Config,
    model_alias: Option<&str>,
) -> crate::error::Result<Option<Llm>> {
    let Some(alias) = model_alias else {
        return Ok(llm.cloned());
    };
    let model = config
        .models
        .get(alias)
        .ok_or_else(|| crate::error::KimiCliError::Config(
            format!("Unknown model alias: {alias}").into()
        ))?;
    let provider = config.providers.get(&model.provider).ok_or_else(|| {
        crate::error::KimiCliError::Config(
            format!("Provider not found for model: {alias}").into()
        )
    })?;
    let thinking = llm.and_then(|l| l.thinking);
    create_llm(provider, model, thinking, None).await
}

/// Derives capabilities for a model based on its name and explicit config.
pub fn derive_model_capabilities(model: &crate::config::LlmModel) -> HashSet<ModelCapability> {
    let mut caps = model.capabilities.clone().unwrap_or_default();
    let name_lower = model.model.to_lowercase();
    if name_lower.contains("thinking") || name_lower.contains("reason") {
        caps.insert(ModelCapability::Thinking);
        caps.insert(ModelCapability::AlwaysThinking);
    } else if model.model == "kimi-for-coding" || model.model == "kimi-code" {
        caps.insert(ModelCapability::Thinking);
        caps.insert(ModelCapability::ImageIn);
        caps.insert(ModelCapability::VideoIn);
    }
    caps
}
