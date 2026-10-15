use std::io::{self, IsTerminal};

use termcolor::{HyperlinkSpec, WriteColor};

/// A writer that supports coloring with either line or block buffering.
#[derive(Debug)]
pub struct StandardStream(StandardStreamKind);

/// Returns a possibly buffered writer to stdout for the given color choice.
///
/// The writer returned is either line buffered or block buffered. The decision
/// between these two is made automatically based on whether a tty is attached
/// to stdout or not. If a tty is attached, then line buffering is used.
/// Otherwise, block buffering is used. In general, block buffering is more
/// efficient, but may increase the time it takes for the end user to see the
/// first bits of output.
///
/// If you need more fine grained control over the buffering mode, then use one
/// of `stdout_buffered_line` or `stdout_buffered_block`.
///
/// The color choice given is passed along to the underlying writer. To
/// completely disable colors in all cases, use `ColorChoice::Never`.
pub fn stdout(color_choice: termcolor::ColorChoice) -> StandardStream {
    if std::io::stdout().is_terminal() {
        stdout_buffered_line(color_choice)
    } else {
        stdout_buffered_block(color_choice)
    }
}

/// Returns a line buffered writer to stdout for the given color choice.
///
/// This writer is useful when printing results directly to a tty such that
/// users see output as soon as it's written. The downside of this approach
/// is that it can be slower, especially when there is a lot of output.
///
/// You might consider using [`stdout`] instead, which chooses the buffering
/// strategy automatically based on whether stdout is connected to a tty.
pub fn stdout_buffered_line(
    color_choice: termcolor::ColorChoice,
) -> StandardStream {
    let out = termcolor::StandardStream::stdout(color_choice);
    StandardStream(StandardStreamKind::LineBuffered(out))
}

/// Returns a block buffered writer to stdout for the given color choice.
///
/// This writer is useful when printing results to a file since it amortizes
/// the cost of writing data. The downside of this approach is that it can
/// increase the latency of display output when writing to a tty.
///
/// You might consider using [`stdout`] instead, which chooses the buffering
/// strategy automatically based on whether stdout is connected to a tty.
pub fn stdout_buffered_block(
    color_choice: termcolor::ColorChoice,
) -> StandardStream {
    // On Unix, wrap stdout in our own 64 KB buffer. termcolor's block-buffered
    // stream uses an 8 KB buffer and its default `write_all`, which means one
    // `write(2)` per 8 KB when printing lots of output (6x more system calls
    // than needed for `rg '' bigfile | ...`). We still decide about color the
    // way termcolor does, by asking a (cheap) line-buffered stream.
    #[cfg(unix)]
    {
        let wtr = io::BufWriter::with_capacity(BLOCK_CAPACITY, io::stdout());
        let kind = if termcolor::StandardStream::stdout(color_choice)
            .supports_color()
        {
            StandardStreamKind::BlockAnsi(termcolor::Ansi::new(wtr))
        } else {
            StandardStreamKind::BlockPlain(termcolor::NoColor::new(wtr))
        };
        StandardStream(kind)
    }
    #[cfg(not(unix))]
    {
        let out = termcolor::BufferedStandardStream::stdout(color_choice);
        StandardStream(StandardStreamKind::BlockBuffered(out))
    }
}

/// The size of the stdout buffer when output is not a terminal.
#[cfg(unix)]
const BLOCK_CAPACITY: usize = 64 * (1 << 10);

#[derive(Debug)]
enum StandardStreamKind {
    LineBuffered(termcolor::StandardStream),
    #[cfg(not(unix))]
    BlockBuffered(termcolor::BufferedStandardStream),
    #[cfg(unix)]
    BlockAnsi(termcolor::Ansi<io::BufWriter<io::Stdout>>),
    #[cfg(unix)]
    BlockPlain(termcolor::NoColor<io::BufWriter<io::Stdout>>),
}

impl io::Write for StandardStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.write(buf),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.write(buf),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.write(buf),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.write(buf),
        }
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.write_all(buf),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.write_all(buf),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.write_all(buf),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.write_all(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.flush(),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.flush(),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.flush(),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.flush(),
        }
    }
}

impl termcolor::WriteColor for StandardStream {
    #[inline]
    fn supports_color(&self) -> bool {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref w) => w.supports_color(),
            #[cfg(not(unix))]
            BlockBuffered(ref w) => w.supports_color(),
            #[cfg(unix)]
            BlockAnsi(ref w) => w.supports_color(),
            #[cfg(unix)]
            BlockPlain(ref w) => w.supports_color(),
        }
    }

    #[inline]
    fn supports_hyperlinks(&self) -> bool {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref w) => w.supports_hyperlinks(),
            #[cfg(not(unix))]
            BlockBuffered(ref w) => w.supports_hyperlinks(),
            #[cfg(unix)]
            BlockAnsi(ref w) => w.supports_hyperlinks(),
            #[cfg(unix)]
            BlockPlain(ref w) => w.supports_hyperlinks(),
        }
    }

    #[inline]
    fn set_color(&mut self, spec: &termcolor::ColorSpec) -> io::Result<()> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.set_color(spec),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.set_color(spec),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.set_color(spec),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.set_color(spec),
        }
    }

    #[inline]
    fn set_hyperlink(&mut self, link: &HyperlinkSpec) -> io::Result<()> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.set_hyperlink(link),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.set_hyperlink(link),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.set_hyperlink(link),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.set_hyperlink(link),
        }
    }

    #[inline]
    fn reset(&mut self) -> io::Result<()> {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref mut w) => w.reset(),
            #[cfg(not(unix))]
            BlockBuffered(ref mut w) => w.reset(),
            #[cfg(unix)]
            BlockAnsi(ref mut w) => w.reset(),
            #[cfg(unix)]
            BlockPlain(ref mut w) => w.reset(),
        }
    }

    #[inline]
    fn is_synchronous(&self) -> bool {
        use self::StandardStreamKind::*;

        match self.0 {
            LineBuffered(ref w) => w.is_synchronous(),
            #[cfg(not(unix))]
            BlockBuffered(ref w) => w.is_synchronous(),
            #[cfg(unix)]
            BlockAnsi(ref w) => w.is_synchronous(),
            #[cfg(unix)]
            BlockPlain(ref w) => w.is_synchronous(),
        }
    }
}
