use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use uwl::Stream;

/// Defines how an operation on an [`Args`] method failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error<E> {
    /// "END-OF-STRING". We reached the end. There's nothing to parse anymore.
    Eos,
    /// Parsing operation failed. Contains how it did.
    Parse(E),
}

impl<E> From<E> for Error<E> {
    fn from(e: E) -> Self {
        Error::Parse(e)
    }
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eos => f.write_str(r#"ArgError("end of string")"#),
            Self::Parse(e) => write!(f, "ArgError(\"{e}\")"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> StdError for Error<E> {}

type Result<T, E> = ::std::result::Result<T, Error<E>>;

/// Dictates how [`Args`] should split arguments, if by one character, or a string.
#[derive(Debug, Clone)]
pub enum Delimiter {
    Single(char),
    Multiple(String),
}

impl Delimiter {
    #[inline]
    fn to_str(&self) -> Cow<'_, str> {
        match self {
            Self::Single(c) => Cow::Owned(c.to_string()),
            Self::Multiple(s) => Cow::Borrowed(s),
        }
    }
}

impl From<char> for Delimiter {
    #[inline]
    fn from(c: char) -> Delimiter {
        Delimiter::Single(c)
    }
}

impl From<String> for Delimiter {
    #[inline]
    fn from(s: String) -> Delimiter {
        Delimiter::Multiple(s)
    }
}

impl<'a> From<&'a String> for Delimiter {
    #[inline]
    fn from(s: &'a String) -> Delimiter {
        Delimiter::Multiple(s.clone())
    }
}

impl<'a> From<&'a str> for Delimiter {
    #[inline]
    fn from(s: &'a str) -> Delimiter {
        Delimiter::Multiple(s.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum TokenKind {
    Argument,
    QuotedArgument,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: TokenKind,
    span: (usize, usize),
}

impl Token {
    #[inline]
    fn new(kind: TokenKind, start: usize, end: usize) -> Self {
        Token {
            kind,
            span: (start, end),
        }
    }
}

// A utility enum to handle an edge case with Apple OSs.
//
// By default, a feature called "Smart Quotes" is enabled on MacOS and iOS devices. This feature
// automatically substitutes the lame, but simple `"` ASCII character for quotation with the cool
// `”` Unicode character. It can be disabled, but users may not want to do that as it is a global
// setting (i.e. they might not want to disable it just for properly invoking commands of bots on
// Discord).
#[derive(Clone, Copy)]
enum QuoteKind {
    Ascii,
    Apple,
}

impl QuoteKind {
    fn new(c: char) -> Option<Self> {
        match c {
            '"' => Some(QuoteKind::Ascii),
            '\u{201C}' => Some(QuoteKind::Apple),
            _ => None,
        }
    }

    fn is_ending_quote(self, c: char) -> bool {
        match self {
            Self::Ascii => c == '"',
            Self::Apple => c == '\u{201D}',
        }
    }
}

fn lex(stream: &mut Stream<'_>, delims: &[Cow<'_, str>]) -> Option<Token> {
    if stream.is_empty() {
        return None;
    }

    let start = stream.offset();
    if let Some(kind) = QuoteKind::new(stream.current_char()?) {
        stream.next_char();

        let mut prev_was_backslash = false;
        stream.take_until_char(|c| {
            let result = kind.is_ending_quote(c) && !prev_was_backslash;
            prev_was_backslash = c == '\\';
            result
        });

        let is_quote = stream.current_char().is_some_and(|c| kind.is_ending_quote(c));
        stream.next_char();

        let end = stream.offset();

        // Remove possible delimiters after the quoted argument.
        for delim in delims {
            stream.eat(delim);
        }

        return Some(if is_quote {
            Token::new(TokenKind::QuotedArgument, start, end)
        } else {
            // We're missing an end quote. View this as a normal argument.
            Token::new(TokenKind::Argument, start, stream.len())
        });
    }

    let mut end = start;

    'outer: while !stream.is_empty() {
        for delim in delims {
            end = stream.offset();

            if stream.eat(delim) {
                break 'outer;
            }
        }

        stream.next_char();
        end = stream.offset();
    }

    Some(Token::new(TokenKind::Argument, start, end))
}

fn is_surrounded_with(s: &str, begin: char, end: char) -> bool {
    s.starts_with(begin) && s.ends_with(end)
}

fn is_quoted(s: &str) -> bool {
    if s.len() < 2 {
        return false;
    }

    // Refer to `QuoteKind` why we check for Unicode quote characters.
    is_surrounded_with(s, '"', '"') || is_surrounded_with(s, '\u{201C}', '\u{201D}')
}

fn strip(s: &str, begin: char, end: char) -> Option<&str> {
    let s = s.strip_prefix(begin)?;
    s.strip_suffix(end)
}

fn remove_quotes(s: &str) -> &str {
    if s.len() < 2 {
        return s;
    }

    if let Some(s) = strip(s, '"', '"') {
        return s;
    }

    // Refer to `QuoteKind` why we check for Unicode quote characters.
    strip(s, '\u{201C}', '\u{201D}').unwrap_or(s)
}

#[derive(Clone, Copy, Debug)]
enum State {
    None,
    Quoted,
    Trimmed,
    // Preserve the order they were called.
    QuotedTrimmed,
    TrimmedQuoted,
}

/// A utility struct for handling "arguments" of a command.
///
/// An "argument" is a part of the message up that ends at one of the specified delimiters, or the
/// end of the message.
///
/// # Example
///
/// ```rust
/// use serenity::framework::standard::{Args, Delimiter};
///
/// let mut args = Args::new("hello world!", &[Delimiter::Single(' ')]); // A space is our delimiter.
///
/// // Parse our argument as a `String` and assert that it's the "hello" part of the message.
/// assert_eq!(args.single::<String>().unwrap(), "hello");
/// // Same here.
/// assert_eq!(args.single::<String>().unwrap(), "world!");
/// ```
///
/// We can also parse "quoted arguments" (no pun intended):
///
/// ```rust
/// use serenity::framework::standard::{Args, Delimiter};
///
/// // Let us imagine this scenario:
/// // You have a `photo` command that grabs the avatar url of a user. This command accepts names only.
/// // Now, one of your users wants the avatar of a user named Princess Zelda.
/// // Problem is, her name contains a space; our delimiter. This would result in two arguments, "Princess" and "Zelda".
/// // So how shall we get around this? Through quotes! By surrounding her name in them we can perceive it as one single argument.
/// let mut args = Args::new(r#""Princess Zelda""#, &[Delimiter::Single(' ')]);
///
/// // Hooray!
/// assert_eq!(args.single_quoted::<String>().unwrap(), "Princess Zelda");
/// ```
///
/// In case of a mistake, we can go back in time... er I mean, one step (or entirely):
///
/// ```rust
/// use serenity::framework::standard::{Args, Delimiter};
///
/// let mut args = Args::new("4 2", &[Delimiter::Single(' ')]);
///
/// assert_eq!(args.single::<u32>().unwrap(), 4);
///
/// // Oh wait, oops, meant to double the 4.
/// // But I won't able to access it now...
/// // oh wait, I can `rewind`.
/// args.rewind();
///
/// assert_eq!(args.single::<u32>().unwrap() * 2, 8);
///
/// // And the same for the 2
/// assert_eq!(args.single::<u32>().unwrap() * 2, 4);
///
/// // WAIT, NO. I wanted to concatenate them into a "42" string...
/// // Argh, what should I do now????
/// // ....
/// // oh, `restore`
/// args.restore();
///
/// let res = format!("{}{}", args.single::<String>().unwrap(), args.single::<String>().unwrap());
///
/// // Yay.
/// assert_eq!(res, "42");
/// ```
///
/// Hmm, taking a glance at the prior example, it seems we have an issue with reading the same
/// argument over and over. Is there a more sensible solution than rewinding...? Actually, there is!
/// The [`Self::current`] and [`Self::parse`] methods:
///
/// ```rust
/// use serenity::framework::standard::{Args, Delimiter};
///
/// let mut args = Args::new("trois cinq quatre six", &[Delimiter::Single(' ')]);
///
/// assert_eq!(args.parse::<String>().unwrap(), "trois");
///
/// // It might suggest we've lost the `trois`. But in fact, we didn't! And not only that, we can do it an infinite amount of times!
/// assert_eq!(args.parse::<String>().unwrap(), "trois");
/// assert_eq!(args.current(), Some("trois"));
/// assert_eq!(args.parse::<String>().unwrap(), "trois");
/// assert_eq!(args.current(), Some("trois"));
///
/// // Only if we use its brother method we'll then lose it.
/// assert_eq!(args.single::<String>().unwrap(), "trois");
/// assert_eq!(args.single::<String>().unwrap(), "cinq");
/// assert_eq!(args.single::<String>().unwrap(), "quatre");
/// assert_eq!(args.single::<String>().unwrap(), "six");
/// ```
#[derive(Clone, Debug)]
pub struct Args {
    message: String,
    tokens: Vec<Token>,
    offset: usize,
    state: State,
}

impl Args {
    /// Create a new instance of [`Args`] for parsing arguments.
    ///
    /// For more reference, look at [`Args`]'s struct documentation.
    ///
    /// # Example
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new(
    /// // Our message from which we'll parse over.
    /// "the quick brown fox jumps over the lazy",
    ///
    /// // The "delimiters", or aka the separators. They denote how we distinguish arguments as their own.
    /// // For this example, we'll use one delimiter, the space (`0x20`), which will separate the message.
    /// &[Delimiter::Single(' ')],
    /// );
    ///
    /// assert_eq!(args.single::<String>().unwrap(), "the");
    /// assert_eq!(args.single::<String>().unwrap(), "quick");
    /// assert_eq!(args.single::<String>().unwrap(), "brown");
    ///
    /// // We shall not see `the quick brown` again.
    /// assert_eq!(args.rest(), "fox jumps over the lazy");
    /// ```
    #[must_use]
    pub fn new(message: &str, possible_delimiters: &[Delimiter]) -> Self {
        let delims = possible_delimiters
            .iter()
            .filter(|d| match d {
                Delimiter::Single(c) => message.contains(*c),
                Delimiter::Multiple(s) => message.contains(s),
            })
            .map(Delimiter::to_str)
            .collect::<Vec<_>>();

        let tokens = if delims.is_empty() {
            let msg = message.trim();
            let kind = if is_quoted(msg) { TokenKind::QuotedArgument } else { TokenKind::Argument };

            if msg.is_empty() {
                Vec::new()
            } else {
                // If there are no delimiters, then the only possible argument is the whole
                // message.
                vec![Token::new(kind, 0, message.len())]
            }
        } else {
            let mut args = Vec::new();
            let mut stream = Stream::new(message);

            while let Some(token) = lex(&mut stream, &delims) {
                // Ignore empty arguments.
                if message[token.span.0..token.span.1].is_empty() {
                    continue;
                }

                args.push(token);
            }

            args
        };

        Args {
            tokens,
            message: message.to_string(),
            offset: 0,
            state: State::None,
        }
    }

    #[inline]
    fn span(&self) -> (usize, usize) {
        self.tokens[self.offset].span
    }

    #[inline]
    fn slice(&self) -> &str {
        let (start, end) = self.span();

        &self.message[start..end]
    }

    /// Move to the next argument. This increments the offset pointer.
    ///
    /// Does nothing if the message is empty.
    pub fn advance(&mut self) -> &mut Self {
        if self.is_empty() {
            return self;
        }

        self.offset += 1;

        self
    }

    /// Go one step behind. This decrements the offset pointer.
    ///
    /// Does nothing if the offset pointer is `0`.
    #[inline]
    pub fn rewind(&mut self) -> &mut Self {
        if self.offset == 0 {
            return self;
        }

        self.offset -= 1;

        self
    }

    /// Go back to the starting point.
    #[inline]
    pub fn restore(&mut self) {
        self.offset = 0;
    }

    fn apply<'a>(&self, s: &'a str) -> &'a str {
        fn trim(s: &str) -> &str {
            let trimmed = s.trim();

            // Search where the argument starts and ends between the whitespace.
            let start = s.find(trimmed).unwrap_or(0);
            let end = start + trimmed.len();

            &s[start..end]
        }

        let mut s = s;

        match self.state {
            State::None => {},
            State::Quoted => {
                s = remove_quotes(s);
            },
            State::Trimmed => {
                s = trim(s);
            },
            State::QuotedTrimmed => {
                s = remove_quotes(s);
                s = trim(s);
            },
            State::TrimmedQuoted => {
                s = trim(s);
                s = remove_quotes(s);
            },
        }

        s
    }

    /// Retrieve the current argument.
    ///
    /// Applies modifications set by [`Self::trimmed`] and [`Self::quoted`].
    ///
    /// # Note
    ///
    /// This borrows [`Args`] for the entire lifetime of the returned argument.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("4 2", &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.current(), Some("4"));
    /// args.advance();
    /// assert_eq!(args.current(), Some("2"));
    /// args.advance();
    /// assert_eq!(args.current(), None);
    /// ```
    #[inline]
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        if self.is_empty() {
            return None;
        }

        let mut s = self.slice();
        s = self.apply(s);

        Some(s)
    }

    /// Apply trimming of whitespace to all arguments.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::Args;
    ///
    /// let mut args = Args::new("     42     ", &[]);
    ///
    /// // trimmed lasts for the whole lifetime of `Args`
    /// args.trimmed();
    /// assert_eq!(args.current(), Some("42"));
    /// // or until we decide ourselves
    /// args.untrimmed();
    /// assert_eq!(args.current(), Some("     42     "));
    /// assert_eq!(args.message(), "     42     ");
    /// ```
    pub fn trimmed(&mut self) -> &mut Self {
        match self.state {
            State::None => self.state = State::Trimmed,
            State::Quoted => self.state = State::QuotedTrimmed,
            _ => {},
        }

        self
    }

    /// Halt trimming of whitespace to all arguments.
    ///
    /// # Examples
    ///
    /// Refer to [`Self::trimmed`]'s examples.
    pub fn untrimmed(&mut self) -> &mut Self {
        match self.state {
            State::Trimmed => self.state = State::None,
            State::QuotedTrimmed | State::TrimmedQuoted => self.state = State::Quoted,
            _ => {},
        }

        self
    }

    /// Remove quotations surrounding all arguments.
    ///
    /// Note that only the quotes of the argument are taken into account. The quotes in the message
    /// are preserved.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::Args;
    ///
    /// let mut args = Args::new("\"42\"", &[]);
    ///
    /// // `quoted` lasts the whole lifetime of `Args`
    /// args.quoted();
    /// assert_eq!(args.current(), Some("42"));
    /// // or until we decide
    /// args.unquoted();
    /// assert_eq!(args.current(), Some("\"42\""));
    /// assert_eq!(args.message(), "\"42\"");
    /// ```
    pub fn quoted(&mut self) -> &mut Self {
        if self.is_empty() {
            return self;
        }

        let is_quoted = self.tokens[self.offset].kind == TokenKind::QuotedArgument;

        if is_quoted {
            match self.state {
                State::None => self.state = State::Quoted,
                State::Trimmed => self.state = State::TrimmedQuoted,
                _ => {},
            }
        }

        self
    }

    /// Stop removing quotations of all arguments.
    ///
    /// # Examples
    ///
    /// Refer to [`Self::quoted`]'s examples.
    pub fn unquoted(&mut self) -> &mut Self {
        match self.state {
            State::Quoted => self.state = State::None,
            State::QuotedTrimmed | State::TrimmedQuoted => self.state = State::Trimmed,
            _ => {},
        }

        self
    }

    /// Parse the current argument.
    ///
    /// Modifications of [`Self::trimmed`] and [`Self::quoted`] are also applied if they were
    /// called.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("4 2", &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.parse::<u32>().unwrap(), 4);
    /// assert_eq!(args.current(), Some("4"));
    /// ```
    ///
    /// # Errors
    ///
    /// May return either [`Error::Parse`] if a parse error occurs, or
    /// [`Error::Eos`] if there are no further remaining args.
    #[inline]
    pub fn parse<T: FromStr>(&self) -> Result<T, T::Err> {
        T::from_str(self.current().ok_or(Error::Eos)?).map_err(Error::Parse)
    }

    /// Parse the current argument and advance.
    ///
    /// Shorthand for calling [`Self::parse`], storing the result, calling [`Self::advance`] and
    /// returning the result.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("4 2", &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.single::<u32>().unwrap(), 4);
    ///
    /// // `4` is now out of the way. Next we have `2`
    /// assert_eq!(args.single::<u32>().unwrap(), 2);
    /// assert!(args.is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// May return the same errors as `parse`.
    #[inline]
    pub fn single<T: FromStr>(&mut self) -> Result<T, T::Err> {
        let p = self.parse::<T>()?;
        self.advance();
        Ok(p)
    }

    /// Remove surrounding quotations, if present, from the argument; parse it and advance.
    ///
    /// Shorthand for `.quoted().single::<T>()`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new(r#""4" "2""#, &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.single_quoted::<String>().unwrap(), "4");
    /// assert_eq!(args.single_quoted::<u32>().unwrap(), 2);
    /// assert!(args.is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// May return the same errors as [`Self::parse`].
    #[inline]
    pub fn single_quoted<T: FromStr>(&mut self) -> Result<T, T::Err> {
        let p = self.quoted().parse::<T>()?;
        self.advance();
        Ok(p)
    }

    /// By starting from the current offset, iterate over any available arguments until there are
    /// none.
    ///
    /// Modifications of [`Iter::trimmed`] and [`Iter::quoted`] are also applied to all arguments if
    /// they were called.
    ///
    /// # Examples
    ///
    /// Assert that all of the numbers in the message are even.
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("4 2", &[Delimiter::Single(' ')]);
    ///
    /// for arg in args.iter::<u32>() {
    ///     // Zero troubles, zero worries.
    ///     let arg = arg.unwrap_or(0);
    ///     assert!(arg % 2 == 0);
    /// }
    ///
    /// assert!(args.is_empty());
    /// ```
    #[inline]
    pub fn iter<T: FromStr>(&mut self) -> Iter<'_, T> {
        Iter {
            args: self,
            state: State::None,
            _marker: PhantomData,
        }
    }

    /// Return an iterator over all unmodified arguments.
    ///
    /// # Examples
    ///
    /// Join the arguments by a comma and a space.
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let args = Args::new("Harry Hermione Ronald", &[Delimiter::Single(' ')]);
    ///
    /// let protagonists = args.raw().collect::<Vec<&str>>().join(", ");
    ///
    /// assert_eq!(protagonists, "Harry, Hermione, Ronald");
    /// ```
    #[inline]
    #[must_use]
    pub fn raw(&self) -> RawArguments<'_> {
        RawArguments {
            tokens: &self.tokens,
            msg: &self.message,
            quoted: false,
        }
    }

    /// Return an iterator over all arguments, stripped of their quotations if any were present.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let args = Args::new("Saw \"The Mist\" \"A Quiet Place\"", &[Delimiter::Single(' ')]);
    ///
    /// let horror_movies = args.raw_quoted().collect::<Vec<&str>>();
    ///
    /// assert_eq!(&*horror_movies, &["Saw", "The Mist", "A Quiet Place"]);
    /// ```
    #[inline]
    #[must_use]
    pub fn raw_quoted(&self) -> RawArguments<'_> {
        let mut raw = self.raw();
        raw.quoted = true;
        raw
    }

    /// Search for any available argument that can be parsed, and remove it from the arguments
    /// queue.
    ///
    /// # Note
    /// The removal is irreversible. And happens after the search *and* the parse were successful.
    ///
    /// # Note 2
    /// "Arguments queue" is the list which contains all arguments that were deemed unique as
    /// defined by quotations and delimiters. The 'removed' argument can be, likewise, still
    /// accessed via [`Self::message`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("c4 2", &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.find::<u32>().unwrap(), 2);
    /// assert_eq!(args.single::<String>().unwrap(), "c4");
    /// assert!(args.is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::Eos`] if no argument can be parsed.
    pub fn find<T: FromStr>(&mut self) -> Result<T, T::Err> {
        if self.is_empty() {
            return Err(Error::Eos);
        }

        let before = self.offset;
        self.restore();

        let Some(pos) = self.iter::<T>().quoted().position(|res| res.is_ok()) else {
            self.offset = before;
            return Err(Error::Eos);
        };

        self.offset = pos;
        let parsed = self.single_quoted::<T>()?;

        self.tokens.remove(pos);
        self.offset = before;
        self.rewind();

        Ok(parsed)
    }

    /// Search for any available argument that can be parsed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use serenity::framework::standard::{Args, Delimiter};
    ///
    /// let mut args = Args::new("c4 2", &[Delimiter::Single(' ')]);
    ///
    /// assert_eq!(args.find_n::<u32>().unwrap(), 2);
    ///
    /// // The `2` is still here, so let's parse it again.
    /// assert_eq!(args.single::<String>().unwrap(), "c4");
    /// assert_eq!(args.single::<u32>().unwrap(), 2);
    /// assert!(args.is_empty());
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`Error::Eos`] if no argument can be parsed.
    pub fn find_n<T: FromStr>(&mut self) -> Result<T, T::Err> {
        if self.is_empty() {
            return Err(Error::Eos);
        }

        let before = self.offset;
        self.restore();

        let Some(pos) = self.iter::<T>().quoted().position(|res| res.is_ok()) else {
            self.offset = before;
            return Err(Error::Eos);
        };

        self.offset = pos;
        let parsed = self.quoted().parse::<T>()?;

        self.offset = before;

        Ok(parsed)
    }

    /// Get the original, unmodified message passed to the command.
    #[inline]
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Starting from the offset, return the remainder of available arguments.
    #[inline]
    #[must_use]
    pub fn rest(&self) -> &str {
        self.remains().unwrap_or_default()
    }

    /// Starting from the offset, return the remainder of available arguments.
    ///
    /// Returns [`None`] if there are no remaining arguments.
    #[inline]
    #[must_use]
    pub fn remains(&self) -> Option<&str> {
        if self.is_empty() {
            return None;
        }

        let (start, _) = self.span();

        Some(&self.message[start..])
    }

    /// Return the full amount of recognised arguments. The length of the "arguments queue".
    ///
    /// # Note
    ///
    /// The value returned is to be assumed to stay static. However, if [`Self::find`] was called
    /// previously, and was successful, then the value is subtracted by one.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Assert that there are no more arguments left.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.offset >= self.len()
    }

    /// Return the amount of arguments still available.
    #[inline]
    #[must_use]
    pub fn remaining(&self) -> usize {
        if self.is_empty() {
            return 0;
        }

        self.len() - self.offset
    }
}

/// Parse each argument individually, as an iterator.
pub struct Iter<'a, T: FromStr> {
    args: &'a mut Args,
    state: State,
    _marker: PhantomData<T>,
}

#[allow(clippy::missing_errors_doc)]
impl<T: FromStr> Iter<'_, T> {
    /// Retrieve the current argument.
    pub fn current(&mut self) -> Option<&str> {
        self.args.state = self.state;
        self.args.current()
    }

    /// Parse the current argument independently.
    pub fn parse(&mut self) -> Result<T, T::Err> {
        self.args.state = self.state;
        self.args.parse::<T>()
    }

    /// Remove surrounding quotation marks from all of the arguments.
    #[inline]
    pub fn quoted(&mut self) -> &mut Self {
        match self.state {
            State::None => self.state = State::Quoted,
            State::Trimmed => self.state = State::TrimmedQuoted,
            _ => {},
        }

        self
    }

    /// Trim leading and trailing whitespace off all arguments.
    #[inline]
    pub fn trimmed(&mut self) -> &mut Self {
        match self.state {
            State::None => self.state = State::Trimmed,
            State::Quoted => self.state = State::QuotedTrimmed,
            _ => {},
        }

        self
    }
}

impl<T: FromStr> Iterator for Iter<'_, T> {
    type Item = Result<T, T::Err>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.args.is_empty() {
            None
        } else {
            let arg = self.parse();
            self.args.advance();
            Some(arg)
        }
    }
}

/// Access to all of the arguments, as an iterator.
#[derive(Debug)]
pub struct RawArguments<'a> {
    msg: &'a str,
    tokens: &'a [Token],
    quoted: bool,
}

impl<'a> Iterator for RawArguments<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let (start, end) = self.tokens.first()?.span;

        self.tokens = &self.tokens[1..];

        let mut s = &self.msg[start..end];

        if self.quoted {
            s = remove_quotes(s);
        }

        Some(s)
    }
}
