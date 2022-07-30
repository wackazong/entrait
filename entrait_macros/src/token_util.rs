use proc_macro2::TokenStream;
use quote::ToTokens;

macro_rules! push_tokens {
    ($stream:expr, $token:expr) => {
        $token.to_tokens($stream)
    };
    ($stream:expr, $token:expr, $($rest:expr),+) => {
        $token.to_tokens($stream);
        push_tokens!($stream, $($rest),*)
    };
}

pub(crate) use push_tokens;

pub struct EmptyToken;

impl quote::ToTokens for EmptyToken {
    fn to_tokens(&self, _: &mut TokenStream) {}
}

pub struct Punctuator<'s, S, P, E: ToTokens> {
    stream: &'s mut TokenStream,
    position: usize,
    start: S,
    punct: P,
    end: E,
}

impl<'s, S, P, E> Punctuator<'s, S, P, E>
where
    S: quote::ToTokens,
    P: quote::ToTokens,
    E: quote::ToTokens,
{
    pub fn new(stream: &'s mut TokenStream, start: S, punct: P, end: E) -> Self {
        Self {
            stream,
            position: 0,
            start,
            punct,
            end,
        }
    }

    pub fn push<T: quote::ToTokens>(&mut self, tokens: T) {
        self.sep();
        tokens.to_tokens(self.stream);
    }

    pub fn push_fn<F>(&mut self, f: F)
    where
        F: FnOnce(&mut TokenStream),
    {
        self.sep();
        f(self.stream);
    }

    fn sep(&mut self) {
        if self.position == 0 {
            self.start.to_tokens(self.stream);
        } else {
            self.punct.to_tokens(self.stream);
        }

        self.position += 1;
    }
}

impl<'s, S, P, E> Drop for Punctuator<'s, S, P, E>
where
    E: quote::ToTokens,
{
    fn drop(&mut self) {
        if self.position > 0 {
            self.end.to_tokens(self.stream);
        }
    }
}
