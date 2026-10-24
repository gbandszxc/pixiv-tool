//! 模型传输与三协议 provider 装配。

pub mod providers;
pub mod transport;

pub use providers::{
    ANTHROPIC_VERSION, ApiFormat, DEFAULT_REASONING_EFFORT, OPENCODE_SESSION_HEADER,
    ProviderConfig, RequestDefaults, apply_request_defaults, completion_endpoint,
    dynamic_output_cap, needs_session_header, normalize_extra, request_defaults,
    resolve_api_format, session_header_for, session_id, validate_extra,
};
pub use transport::WreqTransport;
