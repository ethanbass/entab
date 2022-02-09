use alloc::borrow::Cow;
#[cfg(feature = "std")]
use alloc::boxed::Box;
use alloc::str::Utf8Error;
use alloc::string::{FromUtf8Error, String, ToString};
use alloc::vec::Vec;
use core::convert::Infallible;
use core::fmt;
use core::num::{ParseFloatError, ParseIntError};
#[cfg(feature = "std")]
use std::error::Error;
#[cfg(feature = "std")]
use std::io::Error as IoError;

use crate::buffer::ReadBuffer;

/// Extra information about the error to help identify where in the file being
/// parsed the error occurred.
#[derive(Clone, Debug, Default)]
pub struct EtErrorContext {
    /// At what byte in a the file the error occured
    pub byte: u64,
    /// At what record in a the file the error occured.
    ///
    /// Note, this may not be the same as the index of the iterator
    /// if the underlying file type groups e.g. record information by
    /// time slice.
    pub record: u64,
    /// Buffer content around where the error occured
    pub context: Vec<u8>,
    /// The position in `context` where the error occured
    pub context_pos: usize,
}

#[derive(Debug)]
/// The Error struct for entab
pub struct EtError {
    /// A succinct message describing the error
    pub msg: Cow<'static, str>,
    /// Extra context, if available
    pub context: Option<EtErrorContext>,
    /// If the error could be recovered from by pulling more data into the buffer.
    pub incomplete: bool,
    #[cfg(feature = "std")]
    orig_err: Option<Box<dyn Error>>,
}

impl EtError {
    /// Create a new EtError with a display message of `msg`
    pub fn new(msg: &'static str) -> Self {
        EtError {
            msg: Cow::Borrowed(msg),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: None,
        }
    }

    /// Create a new EtError indicating an incomplete parse state.
    pub fn incomplete(mut self) -> Self {
        self.incomplete = true;
        self
    }

    /// Fill the positional error information from a ReadBuffer
    ///
    /// Used to display e.g. where a parsing error in a file occured.
    pub fn add_context(mut self, buffer: &ReadBuffer) -> Self {
        let buf_len = buffer.as_ref().len();
        let (context, context_pos) = match (buffer.consumed < 16, buf_len < buffer.consumed + 16) {
            (true, true) => (buffer.as_ref().to_vec(), buffer.consumed),
            (true, false) => (
                (&buffer.as_ref()[..buffer.consumed + 16]).to_vec(),
                buffer.consumed,
            ),
            (false, true) => {
                if buffer.consumed < buf_len {
                    ((&buffer.as_ref()[buffer.consumed - 16..]).to_vec(), 16)
                } else {
                    (Vec::new(), 0)
                }
            }
            (false, false) => (
                (&buffer.as_ref()[buffer.consumed - 16..buffer.consumed + 16]).to_vec(),
                16,
            ),
        };

        self.context = Some(EtErrorContext {
            record: buffer.record_pos,
            byte: buffer.reader_pos + buffer.consumed as u64,
            context,
            context_pos,
        });
        self
    }
}

impl fmt::Display for EtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.msg)?;
        if let Some(context) = &self.context {
            for c in &context.context {
                write!(f, "{:X}", c)?;
            }
            writeln!(f)?;
            for c in &context.context {
                if *c > 31 && *c < 127 {
                    write!(f, " {}", char::from(*c))?;
                } else {
                    write!(f, "  ")?;
                }
            }
            write!(
                f,
                "\n{:>width$} {}\n",
                "^^",
                context.byte,
                width = 2 * context.context_pos
            )?;
        };
        Ok(())
    }
}

#[cfg(feature = "std")]
impl Error for EtError {
    fn description(&self) -> &str {
        &self.msg
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.orig_err.as_ref().map(|c| {
            let b: &(dyn Error + 'static) = &**c;
            b
        })
    }
}

impl From<Infallible> for EtError {
    fn from(_error: Infallible) -> Self {
        panic!("Infallible things shouldn't panic!")
    }
}

impl From<&'static str> for EtError {
    fn from(error: &'static str) -> Self {
        EtError {
            msg: Cow::Borrowed(error),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: None,
        }
    }
}

impl From<String> for EtError {
    fn from(msg: String) -> Self {
        EtError {
            msg: Cow::Owned(msg),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: None,
        }
    }
}

impl From<FromUtf8Error> for EtError {
    fn from(error: FromUtf8Error) -> Self {
        EtError {
            msg: Cow::Owned(error.to_string()),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: Some(Box::new(error)),
        }
    }
}

#[cfg(feature = "std")]
impl From<IoError> for EtError {
    fn from(error: IoError) -> Self {
        EtError {
            msg: Cow::Owned(error.to_string()),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: Some(Box::new(error)),
        }
    }
}

impl From<Utf8Error> for EtError {
    fn from(error: Utf8Error) -> Self {
        EtError {
            msg: Cow::Owned(error.to_string()),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: Some(Box::new(error)),
        }
    }
}

impl From<ParseFloatError> for EtError {
    fn from(error: ParseFloatError) -> Self {
        EtError {
            msg: Cow::Owned(error.to_string()),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: Some(Box::new(error)),
        }
    }
}

impl From<ParseIntError> for EtError {
    fn from(error: ParseIntError) -> Self {
        EtError {
            msg: Cow::Owned(error.to_string()),
            context: None,
            incomplete: false,
            #[cfg(feature = "std")]
            orig_err: Some(Box::new(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::format;

    use super::*;

    #[test]
    fn test_context_display() {
        let buf: ReadBuffer = b"1234567890ABCDEF"[..].into();
        let err = EtError::new("Test").add_context(&buf);
        let msg = format!("{}", err);
        assert_eq!(
            msg,
            "Test\
                       \n31323334353637383930414243444546\
                       \n 1 2 3 4 5 6 7 8 9 0 A B C D E F\
                       \n^^ 0\n"
        );

        let mut buf: ReadBuffer = b"1234567890ABCDEF"[..].into();
        buf.consumed += 10;
        let err = EtError::new("Test").add_context(&buf);
        let msg = format!("{}", err);
        assert_eq!(
            msg,
            "Test\
                       \n31323334353637383930414243444546\
                       \n 1 2 3 4 5 6 7 8 9 0 A B C D E F\
                       \n                  ^^ 10\n"
        );
    }
}
