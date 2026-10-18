// Prints the MCP tool list as a client receives it, with its size, to keep it small.
fn main() {
    let tools = filmlab::mcp::tools::definitions();
    let json = serde_json::to_string(&tools).unwrap_or_default();
    if std::env::args().any(|a| a == "--print") {
        println!("{json}");
    }
    eprintln!("{} tools, {} bytes, ~{} tokens", tools.len(), json.len(), json.len() / 4);
}
