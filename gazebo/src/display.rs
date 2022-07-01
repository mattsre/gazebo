/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! Provides utilities to implement `Display` (or `repr`) for Starlark values.
//!
//! For "normal" display, produces output like (with `prefix="prefix[", suffix="]"`):
//!
//! `prefix[]`
//! `prefix[1]`
//! `prefix[1, 2]`
//!
//! For "alternate" display, produces output like:
//!
//! `prefix[]`
//! `prefix[ 1 ]`
//! ```ignore
//! prefix[
//!   1,
//!   2
//! ]
//! ```
//!
//! This doesn't propagate the flags on the Formatter other than alternate.
// TODO(cjhopman): Starlark values don't really do anything with the rest of the flags so
// propagating them hasn't been necessary, but it would be easy enough to implement if we wanted to.

use std::fmt;
use std::fmt::Display;
use std::fmt::Write;

use crate::indenter;

const INDENT: &str = "  ";

/// Used to indent a displayed item for alternate display. This helps us pretty-print deep data structures.
fn subwriter<T: Display>(indent: &'static str, f: &mut fmt::Formatter, v: T) -> fmt::Result {
    if f.alternate() {
        write!(indenter::indented(f, indent), "{:#}", &v)
    } else {
        Display::fmt(&v, f)
    }
}

/// Iterator length for display.
enum Len {
    Zero,
    One,
    Many, // > 1
}

struct PairDisplay<'a, K: Display, V: Display>(K, &'a str, V);
impl<'a, K: Display, V: Display> Display for PairDisplay<'a, K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)?;
        f.write_str(self.1)?;
        Display::fmt(&self.2, f)
    }
}

/// The low-level helper for displaying containers. For simple containers, it may be more convenient to use `display_container` or `display_keyed_container`.
pub struct ContainerDisplayHelper<'a, 'b> {
    f: &'a mut fmt::Formatter<'b>,
    /// The additional separator to be added after each item (except the last).
    separator: &'static str,
    /// Extra output to be added after the prefix and before the suffix. Only non-empty for the single item case.
    outer: &'static str,
    /// The indent used for each item in the multiple items case (where each item is placed on its own line).
    indent: &'static str,
    /// A count of items, used for correctly adding the separator.
    seen_items: usize,
}

impl<'a, 'b> ContainerDisplayHelper<'a, 'b> {
    /// Begins displaying a container. The provided num_items will be used to select which formatting to use for alternate display.
    pub fn begin(
        f: &'a mut fmt::Formatter<'b>,
        prefix: &str,
        num_items: usize,
    ) -> Result<Self, fmt::Error> {
        let num_items = match num_items {
            0 => Len::Zero,
            1 => Len::One,
            _ => Len::Many,
        };
        Self::begin_inner(f, prefix, num_items)
    }

    /// Begins displaying a container. The provided num_items will be used to select which formatting to use for alternate display.
    fn begin_inner(
        f: &'a mut fmt::Formatter<'b>,
        prefix: &str,
        num_items: Len,
    ) -> Result<Self, fmt::Error> {
        let (separator, outer, indent) = match (f.alternate(), num_items) {
            // We want to be formatted as `{prefix}item1, item2{suffix}`, like `[1, 2]` for lists.
            (false, _) => (", ", "", ""),
            // We want to be formatted as `{prefix}{suffix}`, like `[]` for lists.
            (true, Len::Zero) => ("", "", ""),
            // We want to be formatted as `{prefix} item {suffix}`, like `[ item ]` for lists
            (true, Len::One) => ("", " ", ""),
            // We want to be formatted as `{prefix}\n  item1,\n  item2\n{suffix}`, for lists like:
            // ```
            // [
            //    item1,
            //    item2
            // ]
            // ```
            _ => (",\n", "\n", INDENT),
        };
        f.write_str(prefix)?;
        f.write_str(outer)?;

        Ok(Self {
            f,
            separator,
            outer,
            indent,
            seen_items: 0,
        })
    }

    /// Displays an item.
    pub fn item<T: Display>(&mut self, v: T) -> fmt::Result {
        if self.seen_items != 0 {
            self.f.write_str(self.separator)?;
        }
        self.seen_items += 1;
        subwriter(self.indent, self.f, &v)
    }

    /// Displays a keyed item (will be displayed as `<key><separator><value>`)
    pub fn keyed_item<K: Display, V: Display>(
        &mut self,
        k: K,
        separator: &str,
        v: V,
    ) -> fmt::Result {
        self.item(PairDisplay(k, separator, v))
    }

    /// Ends displaying a container.
    pub fn end(self, suffix: &str) -> fmt::Result {
        self.f.write_str(self.outer)?;
        self.f.write_str(suffix)
    }
}

/// Helper for display implementation of starlark container-y types (like list, tuple).
///
/// When displaying a container that produces an ExactSizeIterator, this is more convenient
/// than using `ContainerDisplayHelper` directly.
pub fn display_container<T: Display, Iter: IntoIterator<Item = T>>(
    f: &mut fmt::Formatter,
    prefix: &str,
    suffix: &str,
    items: Iter,
) -> fmt::Result {
    let mut items = items.into_iter();
    let helper = match items.next() {
        None => ContainerDisplayHelper::begin_inner(f, prefix, Len::Zero)?,
        Some(first) => match items.next() {
            None => {
                let mut helper = ContainerDisplayHelper::begin_inner(f, prefix, Len::One)?;
                helper.item(first)?;
                helper
            }
            Some(second) => {
                let mut helper = ContainerDisplayHelper::begin_inner(f, prefix, Len::Many)?;
                helper.item(first)?;
                helper.item(second)?;
                for v in items {
                    helper.item(v)?;
                }
                helper
            }
        },
    };
    helper.end(suffix)
}

/// Helper for display implementation of starlark keyed container-y types (like dict, struct).
///
/// When displaying a keyed container that produces an ExactSizeIterator, this is more convenient
/// than using `ContainerDisplayHelper` directly.
pub fn display_keyed_container<K: Display, V: Display, Iter: IntoIterator<Item = (K, V)>>(
    f: &mut fmt::Formatter,
    prefix: &str,
    suffix: &str,
    separator: &str,
    items: Iter,
) -> fmt::Result {
    display_container(
        f,
        prefix,
        suffix,
        items.into_iter().map(|(k, v)| PairDisplay(k, separator, v)),
    )
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    #[test]
    fn test_container() {
        struct Wrapped(Vec<u32>);
        impl fmt::Display for Wrapped {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                display_container(f, "prefix[", "]", self.0.iter())
            }
        }

        assert_eq!("prefix[]", format!("{:}", Wrapped(vec![])));
        assert_eq!("prefix[1]", format!("{:}", Wrapped(vec![1])));
        assert_eq!("prefix[1, 2]", format!("{:}", Wrapped(vec![1, 2])));
        assert_eq!("prefix[1, 2, 3]", format!("{:}", Wrapped(vec![1, 2, 3])));

        assert_eq!("prefix[]", format!("{:#}", Wrapped(vec![])));
        assert_eq!("prefix[ 1 ]", format!("{:#}", Wrapped(vec![1])));
        assert_eq!(
            "prefix[\n  1,\n  2\n]",
            format!("{:#}", Wrapped(vec![1, 2])),
        );
        assert_eq!(
            "prefix[\n  1,\n  2,\n  3\n]",
            format!("{:#}", Wrapped(vec![1, 2, 3])),
        );
    }

    #[test]
    fn test_keyed_container() {
        struct Wrapped(Vec<(u32, &'static str)>);
        impl fmt::Display for Wrapped {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                display_keyed_container(
                    f,
                    "prefix[",
                    "]",
                    ": ",
                    // just wrap with `"` to make it clearer in the output
                    self.0.iter().map(|(k, v)| (k, format!("\"{}\"", v))),
                )
            }
        }

        assert_eq!("prefix[]", format!("{:}", Wrapped(vec![])));
        assert_eq!("prefix[1: \"1\"]", format!("{:}", Wrapped(vec![(1, "1")])));
        assert_eq!(
            "prefix[1: \"1\", 2: \"2\"]",
            format!("{:}", Wrapped(vec![(1, "1"), (2, "2")])),
        );
        assert_eq!(
            "prefix[1: \"1\", 2: \"2\", 3: \"3\"]",
            format!("{:}", Wrapped(vec![(1, "1"), (2, "2"), (3, "3")])),
        );

        assert_eq!("prefix[]", format!("{:#}", Wrapped(vec![])));
        assert_eq!(
            "prefix[ 1: \"1\" ]",
            format!("{:#}", Wrapped(vec![(1, "1")]))
        );
        assert_eq!(
            "prefix[\n  1: \"1\",\n  2: \"2\"\n]",
            format!("{:#}", Wrapped(vec![(1, "1"), (2, "2")])),
        );
        assert_eq!(
            "prefix[\n  1: \"1\",\n  2: \"2\",\n  3: \"3\"\n]",
            format!("{:#}", Wrapped(vec![(1, "1"), (2, "2"), (3, "3")])),
        );
    }

    #[test]
    fn test_helper() {
        struct Tester;
        impl fmt::Display for Tester {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let mut helper = ContainerDisplayHelper::begin(f, "prefix{", 3)?;
                helper.item("1")?;
                helper.keyed_item("x", "=", 2)?;
                helper.item(3)?;
                helper.end("}")
            }
        }

        assert_eq!("prefix{1, x=2, 3}", format!("{}", Tester));
        assert_eq!("prefix{\n  1,\n  x=2,\n  3\n}", format!("{:#}", Tester));
    }
}
