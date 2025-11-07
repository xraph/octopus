//! Script execution error types

use std::fmt;

/// Script execution result type
pub type Result<T> = std::result::Result<T, ScriptError>;

/// Script execution error
#[derive(Debug, Clone)]
pub enum ScriptError {
    /// Script compilation/parsing error
    CompilationError {
        /// Error message
        message: String,
        /// Line number if available
        line: Option<usize>,
        /// Column number if available
        column: Option<usize>,
    },

    /// Script runtime error
    RuntimeError {
        /// Error message
        message: String,
        /// Script line where error occurred
        line: Option<usize>,
    },

    /// Script timeout
    Timeout {
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Invalid script source
    InvalidSource {
        /// Error message
        message: String,
    },

    /// Unsupported language
    UnsupportedLanguage {
        /// Language name
        language: String,
    },

    /// Script execution panic
    Panic {
        /// Panic message
        message: String,
    },

    /// Type conversion error
    TypeError {
        /// Error message
        message: String,
    },

    /// IO error (reading script files)
    IoError {
        /// Error message
        message: String,
    },
}

impl ScriptError {
    /// Create a compilation error
    pub fn compilation<S: Into<String>>(message: S) -> Self {
        Self::CompilationError {
            message: message.into(),
            line: None,
            column: None,
        }
    }

    /// Create a runtime error
    pub fn runtime<S: Into<String>>(message: S) -> Self {
        Self::RuntimeError {
            message: message.into(),
            line: None,
        }
    }

    /// Create a timeout error
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::Timeout { timeout_ms }
    }

    /// Create an invalid source error
    pub fn invalid_source<S: Into<String>>(message: S) -> Self {
        Self::InvalidSource {
            message: message.into(),
        }
    }

    /// Create an unsupported language error
    pub fn unsupported_language<S: Into<String>>(language: S) -> Self {
        Self::UnsupportedLanguage {
            language: language.into(),
        }
    }

    /// Create a panic error
    pub fn panic<S: Into<String>>(message: S) -> Self {
        Self::Panic {
            message: message.into(),
        }
    }

    /// Create a type error
    pub fn type_error<S: Into<String>>(message: S) -> Self {
        Self::TypeError {
            message: message.into(),
        }
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CompilationError {
                message,
                line,
                column,
            } => {
                write!(f, "Script compilation error: {}", message)?;
                if let Some(line) = line {
                    write!(f, " at line {}", line)?;
                    if let Some(col) = column {
                        write!(f, ", column {}", col)?;
                    }
                }
                Ok(())
            }
            Self::RuntimeError { message, line } => {
                write!(f, "Script runtime error: {}", message)?;
                if let Some(line) = line {
                    write!(f, " at line {}", line)?;
                }
                Ok(())
            }
            Self::Timeout { timeout_ms } => {
                write!(f, "Script timeout after {}ms", timeout_ms)
            }
            Self::InvalidSource { message } => {
                write!(f, "Invalid script source: {}", message)
            }
            Self::UnsupportedLanguage { language } => {
                write!(f, "Unsupported script language: {}", language)
            }
            Self::Panic { message } => {
                write!(f, "Script panic: {}", message)
            }
            Self::TypeError { message } => {
                write!(f, "Script type error: {}", message)
            }
            Self::IoError { message } => {
                write!(f, "Script IO error: {}", message)
            }
        }
    }
}

impl std::error::Error for ScriptError {}

impl From<std::io::Error> for ScriptError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError {
            message: err.to_string(),
        }
    }
}

impl From<Box<rhai::EvalAltResult>> for ScriptError {
    fn from(err: Box<rhai::EvalAltResult>) -> Self {
        let pos = err.position();
        Self::RuntimeError {
            message: err.to_string(),
            line: if pos.is_beginning_of_line() {
                None
            } else {
                Some(pos.line().unwrap_or(0))
            },
        }
    }
}
