//! Replays parser events into a green tree, attaching trivia.

use loom_lexer::Lexed;
use loom_syntax::{GreenNode, GreenNodeBuilder, SyntaxKind};

use crate::parser::{Event, is_significant};

/// Node kinds that take the comments directly above them (doc comments, mostly).
fn attaches_leading_comments(kind: SyntaxKind) -> bool {
    use SyntaxKind::{
        ASSOC_TYPE, CONST_DEF, ENUM_DEF, EXTERN_BLOCK, FIELD_DEF, FN_DEF, IMPL_DEF, MODULE_DEF,
        STRUCT_DEF, TRAIT_DEF, TYPE_ALIAS, USE_DECL, VARIANT_DEF,
    };
    matches!(
        kind,
        FN_DEF
            | STRUCT_DEF
            | ENUM_DEF
            | TRAIT_DEF
            | IMPL_DEF
            | MODULE_DEF
            | USE_DECL
            | CONST_DEF
            | TYPE_ALIAS
            | EXTERN_BLOCK
            | FIELD_DEF
            | VARIANT_DEF
            | ASSOC_TYPE
    )
}

struct Sink<'a> {
    text: &'a str,
    lexed: &'a Lexed,
    /// Index of the next raw token (trivia included) to add.
    raw: usize,
    builder: GreenNodeBuilder,
    open: usize,
}

impl Sink<'_> {
    /// The raw index of the next significant token, or the end.
    fn next_significant(&self) -> usize {
        let tokens = &self.lexed.tokens;
        (self.raw..tokens.len())
            .find(|&index| is_significant(tokens[index].kind))
            .unwrap_or(tokens.len())
    }

    fn add_raw(&mut self, until: usize) {
        while self.raw < until {
            let token = self.lexed.tokens[self.raw];
            self.builder
                .token(token.kind, &self.text[token.range.to_usize()]);
            self.raw += 1;
        }
    }

    /// How many of the trivia tokens in `start..end` (counted from the end) attach to a
    /// following item: comments with no blank line between them and the item.
    fn attached_trivia(&self, start: usize, end: usize) -> usize {
        let tokens = &self.lexed.tokens;
        let mut attached = 0;
        let mut line_breaks = 0;
        for (offset, index) in (start..end).rev().enumerate() {
            match tokens[index].kind {
                SyntaxKind::COMMENT | SyntaxKind::DOC_COMMENT => {
                    attached = offset + 1;
                    line_breaks = 0;
                }
                SyntaxKind::LINE_BREAK | SyntaxKind::NEWLINE => {
                    line_breaks += 1;
                    if line_breaks >= 2 {
                        break;
                    }
                }
                SyntaxKind::WHITESPACE => {}
                _ => break,
            }
        }
        attached
    }

    fn start(&mut self, kind: SyntaxKind) {
        if self.open == 0 {
            self.builder.start_node(kind);
            self.open += 1;
            return;
        }
        let next = self.next_significant();
        if attaches_leading_comments(kind) {
            let attached = self.attached_trivia(self.raw, next);
            self.add_raw(next - attached);
        } else {
            self.add_raw(next);
        }
        self.builder.start_node(kind);
        self.open += 1;
    }

    fn token(&mut self) {
        let next = self.next_significant();
        self.add_raw(next + 1);
    }

    fn finish(&mut self) {
        if self.open == 1 {
            self.add_raw(self.lexed.tokens.len());
        }
        self.builder.finish_node();
        self.open -= 1;
    }
}

pub(crate) fn build(text: &str, lexed: &Lexed, mut events: Vec<Event>) -> GreenNode {
    let mut sink = Sink {
        text,
        lexed,
        raw: 0,
        builder: GreenNodeBuilder::new(),
        open: 0,
    };
    let mut parents = Vec::new();
    for index in 0..events.len() {
        match std::mem::replace(&mut events[index], Event::Tombstone) {
            Event::Start {
                kind,
                forward_parent,
            } => {
                parents.push(kind);
                let mut next = forward_parent.map(|offset| index + offset);
                while let Some(position) = next {
                    match std::mem::replace(&mut events[position], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                        } => {
                            parents.push(kind);
                            next = forward_parent.map(|offset| position + offset);
                        }
                        _ => unreachable!("forward_parent points at a Start event"),
                    }
                }
                while let Some(kind) = parents.pop() {
                    sink.start(kind);
                }
            }
            Event::Finish => sink.finish(),
            Event::Token => sink.token(),
            Event::Tombstone => {}
        }
    }
    sink.builder.finish()
}
