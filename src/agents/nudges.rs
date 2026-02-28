use serde::Deserialize;

/// Blocks EndTurn until TODO checklist is complete (opt-in).
///
/// When present, the agent cannot end its turn without a completed TODO
/// list. `no_todo` fires when no TODO exists at all, `incomplete` fires
/// when items remain unfinished. `{incomplete}` is interpolated with the
/// count.
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressGateConfig {
    pub no_todo: String,
    pub incomplete: String,
}

/// Wall-clock timer nudge, fires after `after_seconds` (opt-in).
///
/// `{minutes}` is interpolated into each message.
///
/// `message` accepts either a single string (used every time) or a list
/// of strings that escalate: index 0 for the first fire, index 1 for the
/// second, and the last element repeats for all subsequent fires.
#[derive(Debug, Clone, Deserialize)]
pub struct TemporalConfig {
    pub after_seconds: u64,
    #[serde(deserialize_with = "deserialize_string_or_vec")]
    pub message: Vec<String>,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrVec;

    impl<'de> de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string or list of strings")
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(vec![v.to_string()])
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut v = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                v.push(s);
            }
            if v.is_empty() {
                Err(de::Error::custom("temporal message list cannot be empty"))
            } else {
                Ok(v)
            }
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

/// Tool-not-used-recently nudge, fires periodically (opt-in).
///
/// Checks the last `window` assistant turns for usage of `tool`. If
/// absent, injects `message`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecencyConfig {
    pub tool: String,
    pub window: usize,
    pub message: String,
}

/// Iteration countdown nudge rule (opt-in).
///
/// Fires when the remaining iterations (`max_iterations - iteration_count`)
/// drops to or below `remaining_iterations`. `{remaining}` is interpolated
/// into `message`.
#[derive(Debug, Clone, Deserialize)]
pub struct IterationCountdownRule {
    pub remaining_iterations: usize,
    pub message: String,
}

/// Context-size threshold nudge, fires once (opt-in).
///
/// When estimated token usage exceeds `threshold_pct` of the model's
/// context window, injects `message`. Uses the actual `input_tokens`
/// from the last provider response as the base, plus estimated tokens
/// for newly appended tool results.
#[derive(Debug, Clone, Deserialize)]
pub struct ContextPressureConfig {
    pub threshold_pct: f64,
    pub message: String,
}
