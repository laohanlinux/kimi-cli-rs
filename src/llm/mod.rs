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
    /// Optional script lines for the `_scripted_echo` provider.
    pub scripted_echo_lines: Vec<String>,
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
            scripted_echo_lines: Vec::new(),
        }
    }
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
        if self.base_url.is_empty()
            && !matches!(
                self.provider_type,
                ProviderType::Echo | ProviderType::ScriptedEcho | ProviderType::Chaos
            )
        {
            return Ok(crate::soul::message::Message {
                role: "assistant".into(),
                content: vec![crate::soul::message::ContentPart::Text {
                    text: "LLM base_url is not configured.".into(),
                }],
                tool_calls: None,
                tool_call_id: None,
            });
        }

        match self.provider_type {
            ProviderType::Echo => self.chat_echo(system_prompt, history),
            ProviderType::ScriptedEcho => self.chat_scripted_echo(),
            ProviderType::Chaos => self.chat_chaos(),
            ProviderType::Anthropic => self.chat_anthropic(system_prompt, history, tools).await,
            _ => {
                self.chat_openai_compatible(system_prompt, history, tools)
                    .await
            }
        }
    }

    fn chat_echo(
        &self,
        system_prompt: Option<&str>,
        history: &[crate::soul::message::Message],
    ) -> crate::error::Result<crate::soul::message::Message> {
        let mut parts = Vec::new();
        if let Some(sp) = system_prompt {
            parts.push(crate::soul::message::ContentPart::Text {
                text: format!("[system]\n{sp}"),
            });
        }
        for msg in history {
            parts.push(crate::soul::message::ContentPart::Text {
                text: format!("[{}]\n{}", msg.role, msg.extract_text("")),
            });
        }
        let text = if parts.is_empty() {
            "echo: no input".into()
        } else {
            parts
                .iter()
                .map(|p| match p {
                    crate::soul::message::ContentPart::Text { text } => text.as_str(),
                    crate::soul::message::ContentPart::Think { thought } => thought.as_str(),
                    _ => "",
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        Ok(crate::soul::message::Message {
            role: "assistant".into(),
            content: vec![crate::soul::message::ContentPart::Text { text }],
            tool_calls: None,
            tool_call_id: None,
        })
    }

    fn chat_scripted_echo(&self) -> crate::error::Result<crate::soul::message::Message> {
        let text = self
            .scripted_echo_lines
            .first()
            .cloned()
            .unwrap_or_else(|| "scripted_echo: end of script".into());
        Ok(crate::soul::message::Message {
            role: "assistant".into(),
            content: vec![crate::soul::message::ContentPart::Text { text }],
            tool_calls: None,
            tool_call_id: None,
        })
    }

    fn chat_chaos(&self) -> crate::error::Result<crate::soul::message::Message> {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let text: String = (0..rng.gen_range(10..50))
            .map(|_| rng.gen_range('a'..='z'))
            .collect();
        Ok(crate::soul::message::Message {
            role: "assistant".into(),
            content: vec![crate::soul::message::ContentPart::Text { text }],
            tool_calls: None,
            tool_call_id: None,
        })
    }

    async fn chat_openai_compatible(
        &self,
        system_prompt: Option<&str>,
        history: &[crate::soul::message::Message],
        tools: Option<&crate::soul::toolset::KimiToolset>,
    ) -> crate::error::Result<crate::soul::message::Message> {
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

        let mut req = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose_secret()),
            );

        if matches!(self.provider_type, ProviderType::Kimi) {
            req = req.header("X-Msh-Context-Caching", "true");
        }

        let response = req.json(&request).send().await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(
                url = %url,
                model = %self.model_name,
                status = status.as_u16(),
                "LLM HTTP error"
            );
            let mut msg = format!("LLM request failed: HTTP {status} - {body}");
            if status == reqwest::StatusCode::NOT_FOUND || body.contains("resource_not_found") {
                msg.push_str(
                    "\n\nHint: (1) `base_url` should be the API host only, e.g. `https://api.moonshot.cn` — \
not `.../v1` (otherwise the client calls `.../v1/v1/chat/completions` and gets 404). \
(2) Check `model` in ~/.kimi/config.toml matches a model id your key can use.",
                );
            }
            return Err(crate::error::KimiCliError::Generic(msg));
        }

        let chat_response: ChatResponse = response.json().await.map_err(|e| {
            crate::error::KimiCliError::Generic(format!("Failed to parse LLM response: {e}"))
        })?;

        let choice = chat_response.choices.into_iter().next().ok_or_else(|| {
            crate::error::KimiCliError::Generic("Empty LLM response choices".into())
        })?;

        Ok(convert_chat_message(&choice.message))
    }

    async fn chat_anthropic(
        &self,
        system_prompt: Option<&str>,
        history: &[crate::soul::message::Message],
        tools: Option<&crate::soul::toolset::KimiToolset>,
    ) -> crate::error::Result<crate::soul::message::Message> {
        let mut messages: Vec<AnthropicMessage> = Vec::new();
        for msg in history {
            if msg.role == "system" {
                continue;
            }
            messages.push(convert_message_to_anthropic(msg));
        }

        let system = system_prompt.map(|s| vec![AnthropicContent::Text { text: s.into() }]);
        let tool_defs = tools.map(|t| convert_tools_to_anthropic(t));

        let request = AnthropicRequest {
            model: self.model_name.clone(),
            max_tokens: 4096,
            system,
            messages,
            tools: tool_defs,
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()?;

        let url = if self.base_url.ends_with('/') {
            format!("{}v1/messages", self.base_url)
        } else {
            format!("{}/v1/messages", self.base_url)
        };

        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", self.api_key.expose_secret())
            .header("anthropic-version", "2023-06-01")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(crate::error::KimiCliError::Generic(format!(
                "Anthropic request failed: HTTP {status} - {body}"
            )));
        }

        let anthropic_response: AnthropicResponse = response.json().await.map_err(|e| {
            crate::error::KimiCliError::Generic(format!("Failed to parse Anthropic response: {e}"))
        })?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();
        for content in anthropic_response.content {
            match content {
                AnthropicContent::Text { text } => text_parts.push(text),
                AnthropicContent::ToolUse { id, name, input } => {
                    tool_calls.push(crate::soul::message::ToolCall {
                        id,
                        name,
                        arguments: input,
                    });
                }
            }
        }

        Ok(crate::soul::message::Message {
            role: "assistant".into(),
            content: vec![crate::soul::message::ContentPart::Text {
                text: text_parts.join("\n"),
            }],
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
            tool_call_id: None,
        })
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

/// Anthropic request payload.
#[derive(Debug, serde::Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<AnthropicContent>>,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<AnthropicToolDef>>,
}

#[derive(Debug, serde::Serialize)]
struct AnthropicMessage {
    role: String,
    content: Vec<AnthropicContent>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
enum AnthropicContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, serde::Serialize)]
struct AnthropicToolDef {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    input_schema: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
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
                    arguments: serde_json::from_str(&c.function.arguments).unwrap_or_else(
                        |_| serde_json::json!({"raw": c.function.arguments.clone()}),
                    ),
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

fn convert_message_to_anthropic(msg: &crate::soul::message::Message) -> AnthropicMessage {
    let mut content = vec![AnthropicContent::Text {
        text: msg.extract_text(""),
    }];
    if let Some(ref calls) = msg.tool_calls {
        for call in calls {
            content.push(AnthropicContent::ToolUse {
                id: call.id.clone(),
                name: call.name.clone(),
                input: call.arguments.clone(),
            });
        }
    }
    AnthropicMessage {
        role: if msg.role == "assistant" {
            "assistant".into()
        } else {
            "user".into()
        },
        content,
    }
}

fn convert_tools_to_anthropic(
    toolset: &crate::soul::toolset::KimiToolset,
) -> Vec<AnthropicToolDef> {
    toolset
        .tools_sync()
        .iter()
        .map(|(name, tool)| AnthropicToolDef {
            name: name.clone(),
            description: Some(tool.description().into()),
            input_schema: tool.parameters_schema(),
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
    let provider_type = provider.r#type.parse().unwrap_or(ProviderType::Kimi);
    let mut capabilities = derive_model_capabilities(model);
    if thinking.unwrap_or(false) {
        capabilities.insert(ModelCapability::Thinking);
    }

    let base_url_raw = augment_base_url_with_env(&provider.base_url, provider_type);
    let base_url = if matches!(
        provider_type,
        ProviderType::Echo | ProviderType::ScriptedEcho | ProviderType::Chaos
    ) {
        base_url_raw
    } else {
        normalize_llm_base_url(&base_url_raw)
    };
    let api_key = augment_api_key_with_env(&provider.api_key, provider_type);

    Ok(Some(Llm {
        model_name: model.model.clone(),
        max_context_size: model.max_context_size,
        capabilities,
        thinking,
        provider_type,
        base_url,
        api_key,
        scripted_echo_lines: Vec::new(),
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
    let model = config.models.get(alias).ok_or_else(|| {
        crate::error::KimiCliError::Config(format!("Unknown model alias: {alias}").into())
    })?;
    let provider = config.providers.get(&model.provider).ok_or_else(|| {
        crate::error::KimiCliError::Config(format!("Provider not found for model: {alias}").into())
    })?;
    let thinking = llm.and_then(|l| l.thinking);
    let mut new_llm = create_llm(provider, model, thinking, None).await?;
    if let Some(ref original) = llm {
        if let Some(ref mut llm) = new_llm {
            llm.scripted_echo_lines = original.scripted_echo_lines.clone();
        }
    }
    Ok(new_llm)
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

/// Strips trailing `/` and a trailing `/v1` so we do not build `.../v1/v1/chat/completions`.
fn normalize_llm_base_url(base: &str) -> String {
    let mut s = base.trim().to_string();
    while s.ends_with('/') {
        s.pop();
    }
    if s.ends_with("/v1") {
        s.truncate(s.len() - 3);
        while s.ends_with('/') {
            s.pop();
        }
    }
    s
}

fn augment_base_url_with_env(base_url: &str, provider_type: ProviderType) -> String {
    let env_var = match provider_type {
        ProviderType::Kimi => "KIMI_BASE_URL",
        ProviderType::Anthropic => "ANTHROPIC_BASE_URL",
        ProviderType::OpenAiLegacy | ProviderType::OpenAiResponses => "OPENAI_BASE_URL",
        ProviderType::GoogleGenAi | ProviderType::Gemini | ProviderType::VertexAi => {
            "GOOGLE_GENAI_BASE_URL"
        }
        _ => return base_url.into(),
    };
    std::env::var(env_var).unwrap_or_else(|_| base_url.into())
}

fn augment_api_key_with_env(
    api_key: &secrecy::SecretString,
    provider_type: ProviderType,
) -> secrecy::SecretString {
    let env_var = match provider_type {
        ProviderType::Kimi => "KIMI_API_KEY",
        ProviderType::Anthropic => "ANTHROPIC_API_KEY",
        ProviderType::OpenAiLegacy | ProviderType::OpenAiResponses => "OPENAI_API_KEY",
        ProviderType::GoogleGenAi | ProviderType::Gemini | ProviderType::VertexAi => {
            "GOOGLE_GENAI_API_KEY"
        }
        _ => return api_key.clone(),
    };
    std::env::var(env_var)
        .map(|v| secrecy::SecretString::new(v.into()))
        .unwrap_or_else(|_| api_key.clone())
}

#[cfg(test)]
mod normalize_tests {
    use super::normalize_llm_base_url;

    #[test]
    fn normalize_strips_trailing_slash() {
        assert_eq!(
            normalize_llm_base_url("https://api.moonshot.cn/"),
            "https://api.moonshot.cn"
        );
    }

    #[test]
    fn normalize_strips_v1_suffix() {
        assert_eq!(
            normalize_llm_base_url("https://api.moonshot.cn/v1"),
            "https://api.moonshot.cn"
        );
        assert_eq!(
            normalize_llm_base_url("https://api.moonshot.cn/v1/"),
            "https://api.moonshot.cn"
        );
    }

    #[test]
    fn normalize_leaves_host_only() {
        assert_eq!(
            normalize_llm_base_url("https://api.moonshot.cn"),
            "https://api.moonshot.cn"
        );
    }
}
