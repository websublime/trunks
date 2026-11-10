//! Standard output of the management commands.
//!
//! `println!` panics when standard output is closed, which is what happens to every command
//! in `trunks list tools | head`. The commands print through [`outln!`] instead: when the
//! reader has gone away there is nobody left to tell, so the command ends quietly.

use std::io::{self, Write as _};

/// Writes one line. Ends the process when it cannot be written: silently and successfully
/// when the reader closed the pipe, with an error otherwise.
pub fn line(args: std::fmt::Arguments<'_>) {
    let mut out = io::stdout().lock();
    if let Err(e) = out.write_fmt(args).and_then(|()| out.write_all(b"\n")) {
        std::process::exit(exit_code(&e));
    }
}

fn exit_code(e: &io::Error) -> i32 {
    if e.kind() == io::ErrorKind::BrokenPipe {
        return 0;
    }
    eprintln!("Error: writing to standard output: {e}");
    1
}

/// `println!` for the commands; see the [module](self) for why.
macro_rules! outln {
    () => {
        $crate::out::line(format_args!(""))
    };
    ($($arg:tt)*) => {
        $crate::out::line(format_args!($($arg)*))
    };
}
pub(crate) use outln;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_pipe_is_not_a_failure_and_anything_else_is() {
        let cases = [
            (io::ErrorKind::BrokenPipe, 0),
            (io::ErrorKind::StorageFull, 1),
            (io::ErrorKind::PermissionDenied, 1),
            (io::ErrorKind::Other, 1),
        ];
        for (kind, expected) in cases {
            assert_eq!(exit_code(&io::Error::from(kind)), expected, "{kind:?}");
        }
    }
}
