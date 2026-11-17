//! Tool-list paging for an MCP server: `tools/list` pages, name validation and

//! conversion into the harness tool schema.

use std::collections::HashSet;

use serde_json::{json, Value};

use orca_harness_core::ToolSchema;

use super::{McpClient, McpError};

pub(super) struct ToolPage {
    pub(super) tools: Vec<(ToolSchema, String)>,
    pub(super) next_cursor: Option<String>,
}

pub(super) fn ensure_unique_tools(tools: &[(ToolSchema, String)]) -> Result<(), McpError> {
    let mut names = HashSet::new();
    if let Some(duplicate) = tools
        .iter()
        .map(|(schema, _)| schema.name.as_str())
        .find(|name| !names.insert(*name))
    {
        return Err(McpError::Protocol(format!(
            "duplicate MCP tool name: {duplicate}"
        )));
    }
    Ok(())
}

pub(super) async fn list_all_tools(
    client: &McpClient,
) -> Result<Vec<(ToolSchema, String)>, McpError> {
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut tools = Vec::new();
    loop {
        let params = cursor
            .as_ref()
            .map_or_else(|| json!({}), |cursor| json!({ "cursor": cursor }));
        let result = client.request("tools/list", params).await?;
        let page = parse_tool_page(client.server(), &result)?;
        tools.extend(page.tools);
        let Some(next) = page.next_cursor else {
            break;
        };
        if !seen.insert(next.clone()) {
            return Err(McpError::Protocol(format!(
                "tools/list repeated cursor: {next}"
            )));
        }
        cursor = Some(next);
    }
    ensure_unique_tools(&tools)?;
    Ok(tools)
}

/// Parse one `tools/list` page. Nameless entries are a protocol violation;
/// entries without a description or input schema get harmless defaults.
pub(super) fn parse_tool_page(server: &str, result: &Value) -> Result<ToolPage, McpError> {
    let object = result
        .as_object()
        .ok_or_else(|| McpError::Protocol("tools/list result is not an object".into()))?;
    let tools = object
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| McpError::Protocol("tools/list returned no tools array".into()))?;
    let tools = tools
        .iter()
        .map(|tool| {
            let remote = tool["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| McpError::Protocol("tools/list entry without a name".into()))?
                .to_string();
            let parameters = tool
                .get("inputSchema")
                .and_then(Value::as_object)
                .map(|schema| Value::Object(schema.clone()))
                .ok_or_else(|| {
                    McpError::Protocol("tools/list entry without an inputSchema object".into())
                })?;
            let schema = ToolSchema {
                name: format!("mcp__{server}__{remote}"),
                description: tool["description"].as_str().unwrap_or("").to_string(),
                parameters,
            };
            Ok((schema, remote))
        })
        .collect::<Result<Vec<_>, McpError>>()?;
    let next_cursor = match object.get("nextCursor") {
        None | Some(Value::Null) => None,
        Some(Value::String(cursor)) if !cursor.is_empty() => Some(cursor.clone()),
        Some(_) => {
            return Err(McpError::Protocol(
                "tools/list nextCursor is not a non-empty string".into(),
            ))
        }
    };
    Ok(ToolPage { tools, next_cursor })
}
