fn main() {
    let code = match nubila::cli::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{}", serde_json::json!({"error": format!("{error:#}")}));
            2
        }
    };
    std::process::exit(code);
}
