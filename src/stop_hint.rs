#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopHintPlatform {
    Unix,
    Windows,
}

pub fn current_platform() -> StopHintPlatform {
    if cfg!(target_os = "windows") {
        StopHintPlatform::Windows
    } else {
        StopHintPlatform::Unix
    }
}

pub fn foreground_stop_hint() -> &'static str {
    foreground_stop_hint_for_platform(current_platform())
}

pub fn background_stop_hint(pid: u32) -> String {
    background_stop_hint_for_platform(pid, current_platform())
}

fn foreground_stop_hint_for_platform(platform: StopHintPlatform) -> &'static str {
    match platform {
        StopHintPlatform::Unix => "To stop monitoring, press Ctrl+C in this terminal.",
        StopHintPlatform::Windows => {
            "To stop monitoring, press Ctrl+C in this PowerShell or Command Prompt window."
        }
    }
}

fn background_stop_hint_for_platform(pid: u32, platform: StopHintPlatform) -> String {
    match platform {
        StopHintPlatform::Unix => format!("To stop monitoring, run: kill {pid}"),
        StopHintPlatform::Windows => {
            format!("To stop monitoring, run in PowerShell: Stop-Process -Id {pid}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_hint_is_unix_aware() {
        assert_eq!(
            foreground_stop_hint_for_platform(StopHintPlatform::Unix),
            "To stop monitoring, press Ctrl+C in this terminal."
        );
    }

    #[test]
    fn foreground_hint_is_windows_aware() {
        assert_eq!(
            foreground_stop_hint_for_platform(StopHintPlatform::Windows),
            "To stop monitoring, press Ctrl+C in this PowerShell or Command Prompt window."
        );
    }

    #[test]
    fn background_hint_is_unix_aware() {
        assert_eq!(
            background_stop_hint_for_platform(12345, StopHintPlatform::Unix),
            "To stop monitoring, run: kill 12345"
        );
    }

    #[test]
    fn background_hint_is_windows_aware() {
        assert_eq!(
            background_stop_hint_for_platform(12345, StopHintPlatform::Windows),
            "To stop monitoring, run in PowerShell: Stop-Process -Id 12345"
        );
    }
}
