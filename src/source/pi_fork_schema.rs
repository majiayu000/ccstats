//! JSONL schema shared by the independently-accounted Pi descendants.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct SessionHeader {
    #[serde(rename = "type")]
    pub(super) entry_type: String,
    pub(super) version: Option<u32>,
    pub(super) id: String,
    pub(super) cwd: String,
    #[serde(rename = "parentSession")]
    pub(super) parent_session: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub(super) enum SessionRecord {
    #[serde(rename = "message")]
    Message {
        id: String,
        timestamp: String,
        message: AssistantEnvelope,
    },
    #[serde(rename = "header_patch")]
    HeaderPatch { patch: HeaderPatch },
    #[serde(rename = "entry_patch")]
    EntryPatch {
        #[serde(rename = "entryId")]
        entry_id: String,
        patch: EntryPatch,
    },
    #[serde(rename = "child_usage_attributed")]
    ChildUsageAttributed {
        #[serde(rename = "targetId")]
        target_id: String,
        #[serde(rename = "childUsage")]
        child_usage: ForkUsage,
        #[serde(rename = "aggregateUsage")]
        aggregate_usage: ForkUsage,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub(super) struct HeaderPatch {
    pub(super) cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EntryPatch {
    pub(super) message: Option<AssistantEnvelope>,
}

#[derive(Debug)]
pub(super) struct SessionEntry {
    pub(super) id: String,
    pub(super) timestamp: String,
    pub(super) message: AssistantEnvelope,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AssistantEnvelope {
    pub(super) role: String,
    pub(super) provider: Option<String>,
    pub(super) model: Option<String>,
    pub(super) timestamp: Option<i64>,
    pub(super) usage: Option<ForkUsage>,
    pub(super) stop_reason: Option<String>,
    pub(super) tool_name: Option<String>,
    pub(super) details: Option<TaskDetails>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ForkUsage {
    pub(super) input: i64,
    pub(super) output: i64,
    pub(super) cache_read: i64,
    pub(super) cache_write: i64,
    #[serde(default)]
    pub(super) reasoning_tokens: i64,
    pub(super) cttl: Option<CacheTtl>,
    pub(super) cost: Option<UsageCost>,
    pub(super) orchestration: Option<OrchestrationUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CacheTtl {
    #[serde(default)]
    pub(super) ephemeral_1h: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct UsageCost {
    pub(super) total: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OrchestrationUsage {
    #[serde(default)]
    pub(super) input: i64,
    #[serde(default)]
    pub(super) output: i64,
    #[serde(default)]
    pub(super) cache_read: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct TaskDetails {
    pub(super) usage: Option<ForkUsage>,
    #[serde(default)]
    pub(super) results: Vec<TaskResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct TaskResult {
    pub(super) id: String,
    pub(super) usage: Option<ForkUsage>,
}

#[derive(Debug, Default)]
pub(super) struct PrimeAttribution {
    pub(super) aggregate: Option<ForkUsage>,
    pub(super) children: Vec<ForkUsage>,
}

pub(super) struct NormalizedUsage {
    pub(super) reasoning: i64,
    pub(super) cache_creation_1h: i64,
    pub(super) recorded_cost_usd: Option<f64>,
}

impl ForkUsage {
    pub(super) fn has_usage(&self) -> bool {
        self.input != 0
            || self.output != 0
            || self.cache_read != 0
            || self.cache_write != 0
            || self.cost.as_ref().is_some_and(|cost| cost.total != 0.0)
    }

    pub(super) fn normalize(
        &mut self,
        has_reasoning: bool,
        include_orchestration: bool,
        record_zero_cost: bool,
    ) -> Result<NormalizedUsage, &'static str> {
        let reasoning = if has_reasoning {
            self.reasoning_tokens
        } else {
            0
        };
        let cache_creation_1h = self.cttl.as_ref().map_or(0, |cttl| cttl.ephemeral_1h);
        if [
            self.input,
            self.output,
            self.cache_read,
            self.cache_write,
            reasoning,
            cache_creation_1h,
        ]
        .into_iter()
        .any(|value| value < 0)
        {
            return Err("negative token count");
        }
        if reasoning > self.output {
            return Err("reasoning exceeds output");
        }
        if cache_creation_1h > self.cache_write {
            return Err("one-hour cache creation exceeds cache write");
        }
        if include_orchestration && let Some(orchestration) = self.orchestration.take() {
            if [
                orchestration.input,
                orchestration.output,
                orchestration.cache_read,
            ]
            .into_iter()
            .any(|value| value < 0)
            {
                return Err("negative orchestration token count");
            }
            self.input = self
                .input
                .checked_add(orchestration.input)
                .ok_or("token count overflow")?;
            self.output = self
                .output
                .checked_add(orchestration.output)
                .ok_or("token count overflow")?;
            self.cache_read = self
                .cache_read
                .checked_add(orchestration.cache_read)
                .ok_or("token count overflow")?;
        }
        let recorded_cost_usd = match self.cost.as_ref().map(|cost| cost.total) {
            None if record_zero_cost => return Err("missing cost"),
            Some(cost) if !cost.is_finite() || cost < 0.0 => return Err("invalid cost"),
            Some(cost) if cost > 0.0 || record_zero_cost => Some(cost),
            _ => None,
        };
        Ok(NormalizedUsage {
            reasoning,
            cache_creation_1h,
            recorded_cost_usd,
        })
    }
}

pub(super) fn subtract_prime_children(
    mut aggregate: ForkUsage,
    children: &[ForkUsage],
) -> Result<ForkUsage, &'static str> {
    for child in children {
        if [
            child.input,
            child.output,
            child.cache_read,
            child.cache_write,
        ]
        .into_iter()
        .any(|value| value < 0)
        {
            return Err("negative child attribution");
        }
        if child.input > aggregate.input
            || child.output > aggregate.output
            || child.cache_read > aggregate.cache_read
            || child.cache_write > aggregate.cache_write
        {
            return Err("child attribution exceeds aggregate usage");
        }
        aggregate.input -= child.input;
        aggregate.output -= child.output;
        aggregate.cache_read -= child.cache_read;
        aggregate.cache_write -= child.cache_write;
        if let (Some(aggregate_cost), Some(child_cost)) =
            (aggregate.cost.as_mut(), child.cost.as_ref())
        {
            if !child_cost.total.is_finite()
                || child_cost.total < 0.0
                || child_cost.total > aggregate_cost.total
            {
                return Err("child attribution exceeds aggregate cost");
            }
            aggregate_cost.total -= child_cost.total;
        } else if child.cost.is_none() {
            aggregate.cost = None;
        }
    }
    aggregate.reasoning_tokens = 0;
    aggregate.cttl = None;
    Ok(aggregate)
}
