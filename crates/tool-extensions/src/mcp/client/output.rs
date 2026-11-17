//! Reduction of MCP `tools/call` results to the model-visible output, lifting

//! image items out so providers can send them as images.

use serde_json::{json, Value};

/// Reduce a `tools/call` result to the model-visible output value:
/// `structuredContent` verbatim when present, text content joined,
/// anything richer passed through raw. Image items move to
/// [`TOOL_IMAGES_KEY`] so providers send them as images; each leaves an
/// `[image N]` marker in the text so interleaved order stays readable.
pub(crate) fn call_output(result: &Value) -> Result<Value, orca_harness_core::ToolError> {
    let object = result
        .as_object()
        .ok_or_else(|| orca_harness_core::ToolError::msg("tools/call result is not an object"))?;
    if object
        .get("isError")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(orca_harness_core::ToolError::msg(
            "tools/call isError is not a boolean",
        ));
    }
    let content = object
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| orca_harness_core::ToolError::msg("tools/call returned no content array"))?;
    let mut images = Vec::new();
    let mut rest = Vec::with_capacity(content.len());
    for item in content {
        match mcp_image(item) {
            Some(Ok(image)) => {
                images.push(image);
                rest.push(json!({ "type": "text", "text": format!("[image {}]", images.len()) }));
            }
            Some(Err(note)) => rest.push(json!({ "type": "text", "text": note })),
            None => rest.push(item.clone()),
        }
    }
    let text = Some(rest.iter().all(|item| item["type"] == "text").then(|| {
        rest.iter()
            .filter_map(|item| item["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }));
    if result["isError"].as_bool() == Some(true) {
        let detail = match text.flatten().filter(|t| !t.is_empty()) {
            Some(text) => text,
            None => "tool reported an error".to_string(),
        };
        return Err(orca_harness_core::ToolError::msg(detail));
    }
    let output = if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null())
    {
        structured.clone()
    } else {
        match text {
            Some(Some(text)) => json!({ "content": text }),
            _ => json!({ "content": rest }),
        }
    };
    if images.is_empty() {
        return Ok(output);
    }
    let mut output = match output {
        Value::Object(object) => object,
        other => serde_json::Map::from_iter([("content".to_string(), other)]),
    };
    output.insert(TOOL_IMAGES_KEY.into(), Value::Array(images));
    Ok(Value::Object(output))
}

/// Output key the model providers read tool images from
/// (`orca_harness_model_providers::tool_images::TOOL_IMAGES_KEY`).
const TOOL_IMAGES_KEY: &str = "_images";

/// Image types every provider accepts as input.
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Largest base64 payload sent per image; providers reject bigger ones.
const MAX_IMAGE_BASE64_BYTES: usize = 5 * 1024 * 1024;

/// An MCP `{"type": "image", "data", "mimeType"}` content item as a
/// `{"media_type", "data"}` image, or a note saying why it was left out.
/// `None` for other items and image items too malformed to recognize.
fn mcp_image(item: &Value) -> Option<Result<Value, String>> {
    if item["type"] != "image" {
        return None;
    }
    let media_type = item["mimeType"].as_str().filter(|m| !m.is_empty())?;
    let data = item["data"].as_str().filter(|d| !d.is_empty())?;
    if !IMAGE_TYPES.contains(&media_type) {
        return Some(Err(format!(
            "[image omitted: {media_type} is not supported]"
        )));
    }
    if data.len() > MAX_IMAGE_BASE64_BYTES {
        return Some(Err(format!(
            "[image omitted: {:.1} MB is over the 5 MB limit]",
            data.len() as f64 / (1024.0 * 1024.0)
        )));
    }
    Some(Ok(json!({ "media_type": media_type, "data": data })))
}
