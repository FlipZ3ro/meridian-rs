use chrono::Local;

const MODULE_WIDTH: usize = 13;
const MODULE_TRUNCATE_AT: usize = MODULE_WIDTH - 2;

pub fn log(level: &str, module: &str, message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let style = std::env::var("MERIDIAN_LOG_STYLE").unwrap_or_else(|_| "pretty".to_string());
    if style.eq_ignore_ascii_case("plain") {
        eprintln!("[{}] [{}/{}] {}", timestamp, level, module, message);
    } else {
        eprintln!("{}", format_log_line(level, module, message, &timestamp));
    }
}

pub fn format_log_line(level: &str, module: &str, message: &str, timestamp: &str) -> String {
    let normalized_level = level.trim().to_ascii_uppercase();
    let icon = level_icon(&normalized_level);
    let module = format_module_column(module);
    format!(
        "{timestamp}  {icon} {level:<5}  {module}│ {message}",
        level = normalized_level,
        module = module,
    )
}

fn level_icon(level: &str) -> &'static str {
    match level {
        "INFO" => "●",
        "WARN" => "▲",
        "ERROR" => "✖",
        "DEBUG" => "·",
        _ => "•",
    }
}

fn format_module_column(module: &str) -> String {
    let trimmed = module.trim();
    if trimmed.chars().count() <= MODULE_WIDTH {
        return format!("{trimmed:<MODULE_WIDTH$}");
    }

    let prefix: String = trimmed.chars().take(MODULE_TRUNCATE_AT).collect();
    let truncated = format!("{prefix}…");
    format!("{truncated:<MODULE_WIDTH$}")
}

pub fn info(message: &str) {
    log("INFO", "app", message);
}
pub fn warn(message: &str) {
    log("WARN", "app", message);
}
pub fn error(message: &str) {
    log("ERROR", "app", message);
}

pub mod module {
    pub fn info(m: &str, msg: &str) {
        super::log("INFO", m, msg);
    }
    pub fn warn(m: &str, msg: &str) {
        super::log("WARN", m, msg);
    }
    pub fn error(m: &str, msg: &str) {
        super::log("ERROR", m, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_log_line_has_icon_columns_and_message_separator() {
        let line = format_log_line("WARN", "screen", "Not enough SOL", "2026-06-09 10:00:00");

        assert_eq!(
            line,
            "2026-06-09 10:00:00  ▲ WARN   screen       │ Not enough SOL"
        );
    }

    #[test]
    fn pretty_log_line_truncates_long_module_names_for_readability() {
        let line = format_log_line(
            "ERROR",
            "very-long-module-name",
            "RPC request failed",
            "2026-06-09 10:00:00",
        );

        assert_eq!(
            line,
            "2026-06-09 10:00:00  ✖ ERROR  very-long-m… │ RPC request failed"
        );
    }

    #[test]
    fn pretty_log_line_keeps_separator_aligned_for_short_and_long_modules() {
        let short = format_log_line("INFO", "main", "started", "2026-06-09 10:00:00");
        let long = format_log_line(
            "INFO",
            "very-long-module-name",
            "started",
            "2026-06-09 10:00:00",
        );

        let short_column = short.chars().position(|ch| ch == '│');
        let long_column = long.chars().position(|ch| ch == '│');
        assert_eq!(short_column, long_column);
    }
}
