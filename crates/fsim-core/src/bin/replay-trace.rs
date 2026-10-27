use fsim_core::{
    trace::{replay, Trace},
    Prototypes,
};
use std::{env, fs, process::ExitCode};

fn run() -> Result<(), String> {
    let mut paths: Vec<String> = env::args().skip(1).collect();
    let data = if paths.first().is_some_and(|s| s == "--prototypes") {
        if paths.len() < 3 {
            return Err("--prototypes needs a snapshot and at least one trace".into());
        }
        let json = fs::read_to_string(&paths[1]).map_err(|e| e.to_string())?;
        paths.drain(..2);
        Prototypes::from_json(&json)?
    } else {
        Prototypes::from_json(include_str!("../../../../data/prototypes.json"))?
    };
    if paths.is_empty() {
        return Err(
            "usage: replay-trace [--prototypes data/prototypes.json] <trace.json>...".into(),
        );
    }
    let mut failures = Vec::new();
    for path in paths {
        let result = (|| {
            let json = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let trace: Trace = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            replay(&trace, &data)
        })();
        match result {
            Ok(report) => println!("{}", serde_json::json!({"path": path, "report": report})),
            Err(error) => failures.push(format!("{path}: {error}")),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("replay-trace: {error}");
            ExitCode::FAILURE
        }
    }
}
