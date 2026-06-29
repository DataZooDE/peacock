//! Parse a Vega-Lite spec's mark + encoding channels into typed structs, and
//! enforce the guardrail (inline data only, no expression escape hatches).

use serde_json::Value;

use crate::RasterError;

/// Keys whose mere presence means a remote fetch or arbitrary computation.
/// Mirrors `peacock-core`'s guardrail so the rasterizer is safe stand-alone.
const FORBIDDEN_KEYS: &[&str] = &["url", "expr", "signal", "signals", "calculate", "loader"];

/// Structurally reject anything outside the safe subset (defence in depth).
pub fn check_guardrail(spec: &Value) -> Result<(), RasterError> {
    match spec {
        Value::Object(map) => {
            for key in map.keys() {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    return Err(RasterError::new(format!(
                        "chart spec uses disallowed feature `{key}` (inline-data-only, no \
                         expressions) — see the safe Vega-Lite subset"
                    )));
                }
            }
            for child in map.values() {
                check_guardrail(child)?;
            }
            Ok(())
        }
        Value::Array(items) => {
            for item in items {
                check_guardrail(item)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Line,
    Bar,
    Point,
    Circle,
    Area,
    Arc,
    Rect,
    Tick,
    Rule,
    Text,
}

impl Mark {
    pub fn parse(spec: &Value) -> Result<Mark, RasterError> {
        let m = spec.get("mark");
        let name = match m {
            Some(Value::String(s)) => s.as_str(),
            Some(Value::Object(o)) => o.get("type").and_then(Value::as_str).unwrap_or(""),
            _ => "",
        };
        match name {
            "line" => Ok(Mark::Line),
            "bar" => Ok(Mark::Bar),
            "point" => Ok(Mark::Point),
            "circle" | "square" => Ok(Mark::Circle),
            "area" => Ok(Mark::Area),
            "arc" => Ok(Mark::Arc),
            "rect" => Ok(Mark::Rect),
            "tick" => Ok(Mark::Tick),
            "rule" => Ok(Mark::Rule),
            "text" => Ok(Mark::Text),
            other => Err(RasterError::new(format!("unsupported mark `{other}`"))),
        }
    }
}

/// A single positional/visual encoding channel.
#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub field: Option<String>,
    /// `quantitative` | `temporal` | `ordinal` | `nominal`.
    pub ty: String,
    pub aggregate: Option<String>,
    pub title: Option<String>,
    pub scale_type: Option<String>,
    pub scale_zero: Option<bool>,
    pub scheme: Option<String>,
    pub range: Option<Vec<String>>,
    pub bin: bool,
    pub sort: Option<Value>,
    /// A literal `value` (constant) instead of a field.
    pub value: Option<Value>,
    pub format: Option<String>,
    pub label_angle: Option<f64>,
    pub grid: Option<bool>,
    pub stack: Option<String>,
}

impl Channel {
    fn from_value(v: &Value) -> Channel {
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_owned);
        let scale = v.get("scale");
        Channel {
            field: s("field"),
            ty: s("type").unwrap_or_default(),
            aggregate: s("aggregate"),
            title: v
                .get("title")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    v.get("title")
                        .filter(|t| t.is_null())
                        .map(|_| String::new())
                }),
            scale_type: scale
                .and_then(|s| s.get("type"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            scale_zero: scale.and_then(|s| s.get("zero")).and_then(Value::as_bool),
            scheme: super::scales::scheme_name(v),
            range: super::scales::explicit_color_range(v),
            bin: v
                .get("bin")
                .map(|b| !b.is_null() && b != &Value::Bool(false))
                .unwrap_or(false),
            sort: v.get("sort").cloned(),
            value: v.get("value").cloned(),
            format: v
                .get("format")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    v.get("axis")
                        .and_then(|a| a.get("format"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                }),
            label_angle: v
                .get("axis")
                .and_then(|a| a.get("labelAngle"))
                .and_then(Value::as_f64),
            grid: v
                .get("axis")
                .and_then(|a| a.get("grid"))
                .and_then(Value::as_bool),
            stack: v.get("stack").and_then(|st| match st {
                Value::String(s) => Some(s.clone()),
                Value::Bool(false) => Some("none".to_owned()),
                Value::Bool(true) => Some("zero".to_owned()),
                Value::Null => Some("none".to_owned()),
                _ => None,
            }),
        }
    }

    pub fn is_temporal(&self) -> bool {
        self.ty == "temporal"
    }
    pub fn is_quantitative(&self) -> bool {
        self.ty == "quantitative"
    }
    pub fn discrete(&self) -> bool {
        matches!(self.ty.as_str(), "ordinal" | "nominal")
            || (self.ty.is_empty() && self.field.is_some())
    }
    /// Effective display title (`None` title key means "no title").
    pub fn effective_title(&self) -> Option<String> {
        match &self.title {
            Some(t) if t.is_empty() => None,
            Some(t) => Some(t.clone()),
            None => self.field.clone(),
        }
    }
}

/// All encoding channels of a (unit) view.
#[derive(Clone, Debug, Default)]
pub struct Encoding {
    pub x: Option<Channel>,
    pub x2: Option<Channel>,
    pub y: Option<Channel>,
    pub y2: Option<Channel>,
    pub color: Option<Channel>,
    pub size: Option<Channel>,
    pub opacity: Option<Channel>,
    pub theta: Option<Channel>,
    pub text: Option<Channel>,
}

impl Encoding {
    pub fn parse(spec: &Value) -> Encoding {
        let enc = match spec.get("encoding") {
            Some(e) => e,
            None => return Encoding::default(),
        };
        let ch = |name: &str| enc.get(name).map(Channel::from_value);
        Encoding {
            x: ch("x"),
            x2: ch("x2"),
            y: ch("y"),
            y2: ch("y2"),
            color: ch("color"),
            size: ch("size"),
            opacity: ch("opacity"),
            theta: ch("theta"),
            text: ch("text"),
        }
    }
}

/// Mark-level style overrides (`mark: {type, color, size, opacity, ...}`).
#[derive(Clone, Debug, Default)]
pub struct MarkDef {
    pub color: Option<String>,
    pub size: Option<f64>,
    pub opacity: Option<f64>,
    pub filled: Option<bool>,
    pub inner_radius: Option<f64>,
    pub point: bool,
}

impl MarkDef {
    pub fn parse(spec: &Value) -> MarkDef {
        let m = spec.get("mark");
        match m {
            Some(Value::Object(o)) => MarkDef {
                color: o.get("color").and_then(Value::as_str).map(str::to_owned),
                size: o.get("size").and_then(Value::as_f64),
                opacity: o.get("opacity").and_then(Value::as_f64),
                filled: o.get("filled").and_then(Value::as_bool),
                inner_radius: o.get("innerRadius").and_then(Value::as_f64),
                point: o.get("point").and_then(Value::as_bool).unwrap_or(false),
            },
            _ => MarkDef::default(),
        }
    }
}
