//! Opt-in machine contract. Plain session JSON deliberately stays unchanged.
use std::{fs::File, io::BufReader, path::Path};

use agent_sessions::{
    AccountingPolicy, Agent, DiscoverFilter, Event, EventKinds, Origin, ReadOptions, Role, Roots,
};
use serde_json::{Value, json};

use crate::{
    app::CommandContext,
    core::{SessionStats, Stats, aggregate_sessions},
    pricing::{PricingDb, calculate_cost, pricing_source_for_model_stats},
    source::{CodexSource, Source, load_entries},
};

#[derive(Default)]
struct NativeMetadata {
    cwd: Option<String>,
    first_user_prompt: Option<String>,
    is_subagent: bool,
    errors: usize,
}

fn metadata(agent: Agent, path: &Path) -> NativeMetadata {
    let mut result = NativeMetadata {
        is_subagent: path
            .components()
            .any(|part| part.as_os_str() == "subagents"),
        ..Default::default()
    };
    let options = ReadOptions {
        include: EventKinds::META.union(EventKinds::MESSAGE),
        accounting: AccountingPolicy::UsageStatistics,
        max_file_bytes: None,
        max_line_bytes: None,
        ..Default::default()
    };
    let reader = File::open(path)
        .map_err(agent_sessions::ReadError::Io)
        .and_then(|file| agent_sessions::read_from(agent, BufReader::new(file), &options));
    let Ok(reader) = reader else {
        result.errors = 1;
        return result;
    };
    for event in reader {
        match event {
            Ok(event) => match event.value {
                Event::Meta(meta) => {
                    if result.cwd.is_none() {
                        result.cwd = meta.cwd.filter(|value| !value.is_empty());
                    }
                    result.is_subagent |= meta.origin == Some(Origin::Subagent);
                }
                Event::Message(message)
                    if message.role == Role::User && result.first_user_prompt.is_none() =>
                {
                    result.first_user_prompt = message
                        .text_segments
                        .iter()
                        .filter_map(|range| message.text.get(range.clone()))
                        .find(|text| !text.is_empty())
                        .map(str::to_owned);
                }
                _ => {}
            },
            Err(_) => result.errors += 1,
        }
        // Metadata is session context, not date-filtered usage. Stop once the
        // first prompt and exact cwd are known instead of retaining transcript text.
        if result.cwd.is_some() && result.first_user_prompt.is_some() {
            break;
        }
    }
    result
}

fn model_json(model: &str, stats: &Stats, pricing: &PricingDb) -> Value {
    let cost = calculate_cost(stats, model, pricing);
    json!({
        "model": model,
        "input_tokens": stats.input_tokens,
        "output_tokens": stats.output_tokens,
        "reasoning_tokens": stats.reasoning_tokens,
        "cache_creation_tokens": stats.cache_creation,
        "cache_creation_1h_tokens": stats.cache_creation_1h,
        "cache_read_tokens": stats.cache_read,
        "requests": stats.count,
        "cost": cost.is_finite().then_some(cost),
        "pricing_source": pricing_source_for_model_stats(model, stats, pricing).as_str(),
    })
}

fn session_json(session: &SessionStats, metadata: &NativeMetadata, pricing: &PricingDb) -> Value {
    let mut models: Vec<_> = session.models.iter().collect();
    models.sort_by_key(|(model, _)| *model);
    json!({
        "session_id": session.session_id,
        "project_path": metadata.cwd.as_deref().unwrap_or(&session.project_path),
        "first_timestamp": session.first_timestamp,
        "last_timestamp": session.last_timestamp,
        "first_user_prompt": metadata.first_user_prompt,
        "is_subagent": metadata.is_subagent,
        "requests": session.stats.count,
        "breakdown": models.into_iter().map(|(model, stats)| model_json(model, stats, pricing)).collect::<Vec<_>>(),
    })
}

pub(crate) fn report(source: &dyn Source, ctx: &CommandContext<'_>) -> Result<Value, String> {
    let agent = match source.name() {
        "claude" => Agent::ClaudeCode,
        "codex" => Agent::Codex,
        _ => return Err("session details only supports Claude and Codex".into()),
    };
    // Ordinary reports historically log discovery errors. Machine consumers need
    // an observable count so an unreadable root cannot turn into a valid zero.
    let roots = Roots::from_env_for(agent).map_err(|error| error.to_string())?;
    let discovery = agent_sessions::discover(
        &roots,
        &DiscoverFilter {
            agents: vec![agent],
            include_subagents: true,
            ..Default::default()
        },
    );
    let diagnostic_source = CodexSource::with_accounting_diagnostics(ctx.cli.codex_scope);
    let accounting_source: &dyn Source = if agent == Agent::Codex {
        &diagnostic_source
    } else {
        source
    };
    let (entries, dedup_skipped, errors) =
        load_entries(accounting_source, ctx.filter, ctx.timezone);
    let mut parse_errors = errors + discovery.errors.len();
    let mut sessions = aggregate_sessions(entries);
    sessions.sort_by(|a, b| a.session_key.cmp(&b.session_key));
    let sessions: Vec<_> = sessions
        .iter()
        .map(|session| {
            let meta = metadata(agent, Path::new(&session.session_key));
            parse_errors += meta.errors;
            session_json(session, &meta, ctx.pricing_db)
        })
        .collect();
    Ok(json!({
        "schema_version": 1,
        "source": source.name(),
        "currency": "USD",
        "cost_kind": "api_equivalent_estimate",
        "parse_errors": parse_errors,
        "dedup_skipped_entries": dedup_skipped,
        "sessions": sessions,
    }))
}
