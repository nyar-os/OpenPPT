//! Command-line entry of OpenPPT.
//!
//! The CLI is a thin adapter: it parses arguments, calls `open-ppt-deck` and
//! prints structured or human output. Deck semantics, transactions, saving and
//! playback stay in the shared upstream model, so the GUI, the Node-API surface
//! and this binary cannot drift apart.

use std::path::Path;

use open_ppt_deck::{CreateRequest, ServiceError};
use serde_json::Value;

const USAGE: &str = "\
Usage: open-ppt <command> [options]

Commands:
  capabilities [--json]
  create --out <file> [--title <title>] [--size 16:9|4:3|16:10] [--slide <title>]... [--overwrite] [--json]
  open <file> [--json]
  inspect <file> [--checks overflow,contrast,alignment,assets,fonts,out-of-bounds] [--json]
  play <file> [--json]

Exit codes: 0 success, 1 inspection errors, 2 usage, 3 deck or file failure.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => std::process::exit(code),
        Err(Failure::Usage(message)) => {
            eprintln!("open-ppt: {message}");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
        Err(Failure::Service(error)) => {
            eprintln!("open-ppt: {error}");
            std::process::exit(3);
        }
    }
}

enum Failure {
    Usage(String),
    Service(ServiceError),
}

impl From<ServiceError> for Failure {
    fn from(error: ServiceError) -> Self {
        Self::Service(error)
    }
}

fn run(args: &[String]) -> Result<i32, Failure> {
    let Some(command) = args.first()
    else {
        return Err(Failure::Usage("no command given".to_string()));
    };
    let flags = Flags::parse(&args[1..])?;
    match command.as_str() {
        "capabilities" => {
            flags.only(&["json"])?;
            let capabilities = open_ppt_deck::capabilities();
            if flags.json() {
                print_json(&capabilities);
            }
            else {
                print_capabilities(&capabilities);
            }
            Ok(0)
        }
        "create" => {
            flags.only(&["out", "title", "size", "slide", "overwrite", "json"])?;
            let out = flags.required("out")?;
            let request = CreateRequest {
                title: flags.value("title").map(str::to_string),
                size: flags.value("size").map(str::to_string),
                slides: flags.values("slide").map(str::to_string).collect(),
            };
            let deck = open_ppt_deck::create(request)?;
            open_ppt_deck::save(&deck, Path::new(out), flags.flag("overwrite"))?;
            let summary = open_ppt_deck::summary(&deck);
            if flags.json() {
                print_json(&serde_json::json!({ "path": out, "deck": summary }));
            }
            else {
                println!("created {out} revision={} slides={}", deck.revision(), deck.slides().len());
            }
            Ok(0)
        }
        "open" => {
            flags.only(&["json"])?;
            let path = flags.required_positional(0)?;
            let deck = open_ppt_deck::open(Path::new(path))?;
            let summary = open_ppt_deck::summary(&deck);
            if flags.json() {
                print_json(&summary);
            }
            else {
                print_summary(&summary);
            }
            Ok(0)
        }
        "inspect" => {
            flags.only(&["checks", "json"])?;
            let path = flags.required_positional(0)?;
            let checks = flags.value("checks").map(open_ppt_deck::parse_checks).transpose()?;
            let deck = open_ppt_deck::open(Path::new(path))?;
            let report = open_ppt_deck::inspect(&deck, checks);
            if flags.json() {
                print_json(&report);
            }
            else {
                print_report(&report);
            }
            Ok(if report["passed"] == Value::Bool(true) { 0 } else { 1 })
        }
        "play" => {
            flags.only(&["json"])?;
            let path = flags.required_positional(0)?;
            let deck = open_ppt_deck::open(Path::new(path))?;
            let frames = open_ppt_deck::play(&deck)?;
            if flags.json() {
                print_json(&serde_json::json!({ "frames": frames }));
            }
            else {
                for (index, frame) in frames.iter().enumerate() {
                    let slide = frame["slideId"].as_str().unwrap_or_default();
                    let notes = frame["notes"].as_str().map(|notes| format!(" notes={notes}")).unwrap_or_default();
                    println!("{index}: slide={slide}{notes}");
                }
            }
            Ok(0)
        }
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(0)
        }
        other => Err(Failure::Usage(format!("unknown command `{other}`"))),
    }
}

fn print_json(value: &Value) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()));
}

fn print_capabilities(capabilities: &Value) {
    println!(
        "engine: {} {}",
        capabilities["engine"]["package"].as_str().unwrap_or_default(),
        capabilities["engine"]["version"].as_str().unwrap_or_default()
    );
    let project = &capabilities["project"];
    println!(
        "project: import={} edit={} export={} playback={}",
        project["import"].as_str().unwrap_or_default(),
        project["edit"].as_str().unwrap_or_default(),
        project["export"].as_str().unwrap_or_default(),
        project["playback"].as_str().unwrap_or_default()
    );
    if let Some(formats) = capabilities["formats"].as_object() {
        let supported: Vec<&str> = formats
            .iter()
            .filter(|(_, entry)| entry["import"].as_str() == Some("supported"))
            .map(|(name, _)| name.as_str())
            .collect();
        if supported.is_empty() {
            println!("presentation formats: none supported");
        }
        else {
            println!("presentation formats: {}", supported.join(", "));
        }
    }
    if let Some(checks) = capabilities["checks"].as_object() {
        let supported: Vec<&str> =
            checks.iter().filter(|(_, level)| level.as_str() == Some("supported")).map(|(name, _)| name.as_str()).collect();
        println!("checks: {}", supported.join(", "));
    }
    let playback = &capabilities["playback"];
    println!("playback: staticSlides={}", playback["staticSlides"].as_str().unwrap_or_default());
}

fn print_summary(summary: &Value) {
    println!(
        "deck {} revision={} slides={} size={}x{}pt",
        summary["id"].as_str().unwrap_or_default(),
        summary["revision"],
        summary["slides"].as_array().map_or(0, Vec::len),
        summary["size"]["widthPt"],
        summary["size"]["heightPt"]
    );
    if let Some(slides) = summary["slides"].as_array() {
        for slide in slides {
            println!(
                "  [{}] {} {}{}",
                slide["index"],
                slide["id"].as_str().unwrap_or_default(),
                slide["title"].as_str().unwrap_or_default(),
                if slide["hidden"] == Value::Bool(true) { " (hidden)" } else { "" }
            );
        }
    }
}

fn print_report(report: &Value) {
    println!(
        "inspect revision={} checker={} passed={}",
        report["revision"],
        report["checkerVersion"].as_str().unwrap_or_default(),
        report["passed"]
    );
    if let Some(issues) = report["issues"].as_array() {
        for issue in issues {
            println!(
                "  {} {} {} {}",
                issue["severity"].as_str().unwrap_or_default(),
                issue["code"].as_str().unwrap_or_default(),
                issue["slideId"].as_str().unwrap_or("-"),
                issue["message"].as_str().unwrap_or_default()
            );
        }
    }
}

struct Flags {
    values: Vec<(String, String)>,
    positionals: Vec<String>,
}

impl Flags {
    fn parse(args: &[String]) -> Result<Self, Failure> {
        let mut values = Vec::new();
        let mut positionals = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if let Some(name) = arg.strip_prefix("--") {
                if matches!(name, "json" | "overwrite") {
                    values.push((name.to_string(), "true".to_string()));
                    index += 1;
                }
                else {
                    let value = args.get(index + 1).ok_or_else(|| Failure::Usage(format!("`--{name}` needs a value")))?;
                    values.push((name.to_string(), value.clone()));
                    index += 2;
                }
            }
            else {
                positionals.push(arg.clone());
                index += 1;
            }
        }
        Ok(Self { values, positionals })
    }

    fn only(&self, allowed: &[&str]) -> Result<(), Failure> {
        if let Some((name, _)) = self.values.iter().find(|(name, _)| !allowed.contains(&name.as_str())) {
            return Err(Failure::Usage(format!("unknown option `--{name}`")));
        }
        Ok(())
    }

    fn flag(&self, name: &str) -> bool {
        self.values.iter().any(|(key, _)| key == name)
    }

    fn json(&self) -> bool {
        self.flag("json")
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> {
        self.values.iter().filter(move |(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn required(&self, name: &str) -> Result<&str, Failure> {
        self.value(name).ok_or_else(|| Failure::Usage(format!("`--{name}` is required")))
    }

    fn required_positional(&self, index: usize) -> Result<&str, Failure> {
        self.positionals.get(index).map(String::as_str).ok_or_else(|| Failure::Usage("a file path is required".to_string()))
    }
}
