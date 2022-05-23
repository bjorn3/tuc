use anyhow::{bail, Result};
use std::cmp::Ordering;
use std::fmt;
use std::ops::Range;
use std::str::FromStr;

#[derive(Debug, PartialEq)]
pub enum BoundsType {
    Bytes,
    Characters,
    Fields,
    Lines,
}

#[derive(Debug, PartialEq)]
pub enum BoundOrFiller {
    Bound(UserBounds),
    Filler(String),
}

/**
 * Parse bound string. It can contain formatting elements or not.
 *
 * Valid bounds formats are e.g. 1 / -1 / 1:3 / :3 / 1: / 1,4
 * If '{' is present, the string is considered to be a format string:
 * in that case everything inside {} is considered a bound, and the rest
 * just some text to display when the bounds are found.
 * e.g. "Hello {1}, found {1:3} and {2,4}"
 */
fn parse_bounds_list(s: &str) -> Result<Vec<BoundOrFiller>> {
    if s.contains(&['{', '}']) {
        if s.len() == 1 {
            if s == "{" {
                bail!("Field format error: missing closing parenthesis");
            } else {
                bail!("Field format error: missing opening parenthesis");
            }
        }

        let esc_open = "__tuc_open";
        let esc_close = "__tuc_close";
        let s = s
            .replace("{{", esc_open)
            .replace("}}", esc_close)
            .replace("\\n", "\n");

        let mut v: Vec<BoundOrFiller> = Vec::new();
        let mut prev_filler_start = 0;
        let mut bound_idx_start = None;
        let mut bound_idx_end = None;
        for (i, c) in s.char_indices() {
            if c == '}' && bound_idx_start.is_none() {
                bail!("Field format error: missing opening parenthesis");
            }
            if c == '{' && bound_idx_start.is_some() {
                bail!("Field format error: missing closing parenthesis");
            }
            if c == '{' {
                bound_idx_start = Some(i);
            } else if c == '}' {
                if let Some(filler) = s.get(prev_filler_start..bound_idx_start.unwrap()) {
                    if !filler.is_empty() {
                        v.push(BoundOrFiller::Filler(
                            filler
                                .to_owned()
                                .replace(esc_open, "{{")
                                .replace(esc_close, "}}"),
                        ));
                    }
                    prev_filler_start = i + 1;
                }

                // handle comma separated bounds
                for maybe_bounds in s[bound_idx_start.unwrap() + 1..i].split(',') {
                    v.push(BoundOrFiller::Bound(UserBounds::from_str(maybe_bounds)?));
                }

                bound_idx_start = None;
                bound_idx_end = Some(i);
            }
        }

        if bound_idx_start.is_some() {
            bail!("Field format error: missing closing parenthesis");
        }

        if let Some(last_bound_idx_end) = bound_idx_end {
            if last_bound_idx_end < s.len() - 1 {
                v.push(BoundOrFiller::Filler(
                    s[last_bound_idx_end + 1..s.len()]
                        .to_owned()
                        .replace(esc_open, "{{")
                        .replace(esc_close, "}}"),
                ));
            }
        }

        Ok(v)
    } else {
        let k: Result<Vec<BoundOrFiller>, _> = s
            .split(',')
            .map(|x| UserBounds::from_str(x).map(BoundOrFiller::Bound))
            .collect();
        Ok(k?)
    }
}

pub fn parse_bounds_list_2(s: &str) -> Result<Vec<BoundOrFiller>> {
    if s.contains(&['{', '}']) {
        let mut bof: Vec<BoundOrFiller> = Vec::new();
        let mut inside_bound = false;
        let mut part_start = 0;

        let mut iter = s.chars().enumerate().peekable();
        while let Some((idx, w0)) = iter.next() {
            let w1 = iter.peek().or(Some(&(0, 'x'))).unwrap().1;

            if w0 == w1 && (w0 == '{' || w0 == '}') {
                // escaped bracket, ignore it, we will replace it later
                iter.next();
            } else if w0 == '}' && !inside_bound {
                bail!("Field format error: missing opening parenthesis",);
            } else if w0 == '{' {
                // starting a new bound
                inside_bound = true;

                if idx - part_start > 0 {
                    bof.push(BoundOrFiller::Filler(s[part_start..idx].to_string()));
                }

                part_start = idx + 1;
            } else if w0 == '}' {
                // ending a bound
                inside_bound = false;

                // consider also comma separated bounds
                for maybe_bounds in s[part_start..idx].split(',') {
                    bof.push(BoundOrFiller::Bound(UserBounds::from_str(maybe_bounds)?));
                }

                part_start = idx + 1;
            }
        }

        if inside_bound {
            bail!("Field format error: missing closing parenthesis");
        } else if s.len() - part_start > 0 {
            bof.push(BoundOrFiller::Filler(s[part_start..].to_string()));
        }

        Ok(bof)
    } else {
        let k: Result<Vec<BoundOrFiller>, _> = s
            .split(',')
            .map(|x| UserBounds::from_str(x).map(BoundOrFiller::Bound))
            .collect();
        Ok(k?)
    }
}

#[derive(Debug)]
pub struct UserBoundsList(pub Vec<BoundOrFiller>);

impl FromStr for UserBoundsList {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(UserBoundsList(parse_bounds_list(s)?))
    }
}

impl UserBoundsList {
    pub fn is_sortable(&self) -> bool {
        let mut has_positive_idx = false;
        let mut has_negative_idx = false;
        self.0
            .iter()
            .flat_map(|b| match b {
                BoundOrFiller::Bound(x) => Some(x),
                _ => None,
            })
            .for_each(|b| {
                if let Side::Some(left) = b.l {
                    if left.is_positive() {
                        has_positive_idx = true;
                    } else {
                        has_negative_idx = true;
                    }
                }

                if let Side::Some(right) = b.r {
                    if right.is_positive() {
                        has_positive_idx = true;
                    } else {
                        has_negative_idx = true;
                    }
                }
            });

        !(has_negative_idx && has_positive_idx)
    }

    pub fn is_sorted(&self) -> bool {
        self.0.windows(2).all(|w| match (&w[0], &w[1]) {
            (BoundOrFiller::Bound(x), BoundOrFiller::Bound(y)) => x <= y,
            _ => true,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Some(i32),
    Continue,
}

impl FromStr for Side {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "" => Side::Continue,
            _ => Side::Some(
                s.parse::<i32>()
                    .or_else(|_| bail!("Not a number `{}`", s))?,
            ),
        })
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Side::Some(v) => write!(f, "{}", v),
            Side::Continue => write!(f, ""),
        }
    }
}

#[derive(Debug, Eq, Clone)]
pub struct UserBounds {
    pub l: Side,
    pub r: Side,
}

impl fmt::Display for UserBounds {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self.l, self.r) {
            (Side::Continue, Side::Continue) => write!(f, "1:-1"),
            (l, r) if l == r => write!(f, "{}", l),
            (l, r) => write!(f, "{}:{}", l, r),
        }
    }
}

impl FromStr for UserBounds {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            bail!("Field format error: empty field");
        } else if s == ":" {
            bail!("Field format error, no numbers next to `:`");
        }

        let (l, r) = match s.find(':') {
            None => {
                let side = Side::from_str(s)?;
                (side, side)
            }
            Some(idx_colon) if idx_colon == 0 => {
                (Side::Continue, Side::from_str(&s[idx_colon + 1..])?)
            }
            Some(idx_colon) if idx_colon == s.len() - 1 => {
                (Side::from_str(&s[..idx_colon])?, Side::Continue)
            }
            Some(idx_colon) => (
                Side::from_str(&s[..idx_colon])?,
                Side::from_str(&s[idx_colon + 1..])?,
            ),
        };

        match (l, r) {
            (Side::Some(0), _) => {
                bail!("Field value 0 is not allowed (fields are 1-indexed)");
            }
            (_, Side::Some(0)) => {
                bail!("Field value 0 is not allowed (fields are 1-indexed)");
            }
            (Side::Some(left), Side::Some(right)) if right < left => {
                bail!("Field left value cannot be greater than right value");
            }
            _ => (),
        }

        Ok(UserBounds::new(l, r))
    }
}

impl UserBounds {
    pub fn new(l: Side, r: Side) -> Self {
        UserBounds { l, r }
    }
    /**
     * Check if an index is between the bounds.
     *
     * It errors out if the index has different sign than the bounds
     * (we can't verify if e.g. -1 idx is between 3:5 without knowing the number
     * of matching bounds).
     */
    pub fn matches(&self, idx: i32) -> Result<bool> {
        match (self.l, self.r) {
            (Side::Some(left), _) if (left * idx).is_negative() => {
                bail!(
                    "sign mismatch. Can't verify if index {} is between bounds {}",
                    idx,
                    self
                )
            }
            (_, Side::Some(right)) if (right * idx).is_negative() => {
                bail!(
                    "sign mismatch. Can't verify if index {} is between bounds {}",
                    idx,
                    self
                )
            }
            (Side::Continue, Side::Continue) => Ok(true),
            (Side::Some(left), Side::Some(right)) if left <= idx && idx <= right => Ok(true),
            (Side::Continue, Side::Some(right)) if idx <= right => Ok(true),
            (Side::Some(left), Side::Continue) if left <= idx => Ok(true),
            _ => Ok(false),
        }
    }
}

impl Ord for UserBounds {
    /*
     * Compare UserBounds. Note that comparison gives wrong results if
     * bounds happen to have a mix of positive/negative indexes (you cannot
     * reliably compare -1 with 3 without kwowing how many parts are there).
     * Check with UserBounds.is_sortable before comparing.
     */
    fn cmp(&self, other: &Self) -> Ordering {
        if self == other {
            return Ordering::Equal;
        }

        match (self.l, self.r, other.l, other.r) {
            (_, Side::Some(s_r), Side::Some(o_l), _) if (s_r * o_l).is_positive() && s_r <= o_l => {
                Ordering::Less
            }
            _ => Ordering::Greater,
        }
    }
}

impl PartialOrd for UserBounds {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for UserBounds {
    fn eq(&self, other: &Self) -> bool {
        (self.l, self.r) == (other.l, other.r)
    }
}

impl Default for UserBounds {
    fn default() -> Self {
        UserBounds::new(Side::Some(1), Side::Some(1))
    }
}

pub fn bounds_to_std_range(parts_length: usize, bounds: &UserBounds) -> Result<Range<usize>> {
    let start: usize = match bounds.l {
        Side::Continue => 0,
        Side::Some(v) => {
            if v.abs() as usize > parts_length {
                bail!("Out of bounds: {}", v);
            }
            if v < 0 {
                parts_length - v.abs() as usize
            } else {
                v as usize - 1
            }
        }
    };

    let end: usize = match bounds.r {
        Side::Continue => parts_length,
        Side::Some(v) => {
            if v.abs() as usize > parts_length {
                bail!("Out of bounds: {}", v);
            }
            if v < 0 {
                parts_length - v.abs() as usize + 1
            } else {
                v as usize
            }
        }
    };

    Ok(Range { start, end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_bounds_formatting() {
        assert_eq!(
            UserBounds::new(Side::Continue, Side::Continue).to_string(),
            "1:-1"
        );
        assert_eq!(
            UserBounds::new(Side::Continue, Side::Some(3)).to_string(),
            ":3"
        );
        assert_eq!(
            UserBounds::new(Side::Some(3), Side::Continue).to_string(),
            "3:"
        );
        assert_eq!(
            UserBounds::new(Side::Some(1), Side::Some(2)).to_string(),
            "1:2"
        );
        assert_eq!(
            UserBounds::new(Side::Some(-1), Side::Some(-2)).to_string(),
            "-1:-2"
        );
    }

    #[test]
    fn test_user_bounds_from_str() {
        assert_eq!(
            UserBounds::from_str("1").ok(),
            Some(UserBounds::new(Side::Some(1), Side::Some(1))),
        );
        assert_eq!(
            UserBounds::from_str("-1").ok(),
            Some(UserBounds::new(Side::Some(-1), Side::Some(-1))),
        );
        assert_eq!(
            UserBounds::from_str("1:2").ok(),
            Some(UserBounds::new(Side::Some(1), Side::Some(2))),
        );
        assert_eq!(
            UserBounds::from_str("-2:-1").ok(),
            Some(UserBounds::new(Side::Some(-2), Side::Some(-1))),
        );
        assert_eq!(
            UserBounds::from_str("1:").ok(),
            Some(UserBounds::new(Side::Some(1), Side::Continue)),
        );
        assert_eq!(
            UserBounds::from_str("-1:").ok(),
            Some(UserBounds::new(Side::Some(-1), Side::Continue)),
        );
        assert_eq!(
            UserBounds::from_str(":1").ok(),
            Some(UserBounds::new(Side::Continue, Side::Some(1))),
        );
        assert_eq!(
            UserBounds::from_str(":-1").ok(),
            Some(UserBounds::new(Side::Continue, Side::Some(-1))),
        );

        {
            #![allow(clippy::bind_instead_of_map)]
            assert_eq!(
                UserBounds::from_str("2:1")
                    .err()
                    .and_then(|x| Some(x.to_string())),
                Some(String::from(
                    "Field left value cannot be greater than right value"
                ))
            );
            assert_eq!(
                UserBounds::from_str("-1:-2")
                    .err()
                    .and_then(|x| Some(x.to_string())),
                Some(String::from(
                    "Field left value cannot be greater than right value"
                ))
            );
        }
    }

    #[test]
    fn test_parse_bounds_list() {
        // do not replicate tests from test_user_bounds_from_str, focus on
        // multiple bounds, bounds with format, and special cases (empty/one)

        assert_eq!(
            &parse_bounds_list("").unwrap_err().to_string(),
            "Field format error: empty field"
        );

        assert_eq!(
            &parse_bounds_list("{").unwrap_err().to_string(),
            "Field format error: missing closing parenthesis"
        );

        assert_eq!(
            &parse_bounds_list("}").unwrap_err().to_string(),
            "Field format error: missing opening parenthesis"
        );

        assert_eq!(
            &parse_bounds_list("{1}{").unwrap_err().to_string(),
            "Field format error: missing closing parenthesis"
        );

        // TODO these are going to give confusing error messages because
        // we transform {{ and }} internally and the error message looks like
        // the opposite case is happening (missing open/closed). At least it
        // must return an error, in future we should parse properly the format
        // string.
        assert!(parse_bounds_list("{1}}").is_err());
        assert!(parse_bounds_list("{{1}").is_err());

        assert_eq!(
            parse_bounds_list("1").unwrap(),
            vec![BoundOrFiller::Bound(UserBounds::new(
                Side::Some(1),
                Side::Some(1)
            ))],
        );

        assert_eq!(
            parse_bounds_list("{1}").unwrap(),
            vec![BoundOrFiller::Bound(UserBounds::new(
                Side::Some(1),
                Side::Some(1)
            ))],
        );

        assert_eq!(
            parse_bounds_list("{1:2}").unwrap(),
            vec![BoundOrFiller::Bound(UserBounds::new(
                Side::Some(1),
                Side::Some(2)
            ))],
        );

        assert_eq!(
            parse_bounds_list("{1,2}").unwrap(),
            vec![
                BoundOrFiller::Bound(UserBounds::new(Side::Some(1), Side::Some(1))),
                BoundOrFiller::Bound(UserBounds::new(Side::Some(2), Side::Some(2)))
            ],
        );

        assert_eq!(
            parse_bounds_list("hello {1,2} world").unwrap(),
            vec![
                BoundOrFiller::Filler(String::from("hello ")),
                BoundOrFiller::Bound(UserBounds::new(Side::Some(1), Side::Some(1))),
                BoundOrFiller::Bound(UserBounds::new(Side::Some(2), Side::Some(2))),
                BoundOrFiller::Filler(String::from(" world")),
            ],
        );
    }

    #[test]
    fn test_user_bounds_is_sortable() {
        assert!(UserBoundsList(Vec::new()).is_sortable());

        assert!(UserBoundsList(vec![BoundOrFiller::Bound(
            UserBounds::from_str("1").unwrap()
        ),])
        .is_sortable());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("2").unwrap()),
        ])
        .is_sortable());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("3").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("2").unwrap()),
        ])
        .is_sortable());

        assert!(!UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("-1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
        ])
        .is_sortable());

        assert!(!UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("-1:").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str(":1").unwrap()),
        ])
        .is_sortable());
    }

    #[test]
    fn test_vec_of_bounds_is_sorted() {
        assert!(UserBoundsList(vec![BoundOrFiller::Bound(
            UserBounds::from_str("1").unwrap()
        ),])
        .is_sorted());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("2").unwrap()),
        ])
        .is_sorted());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("-2").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("-1").unwrap()),
        ])
        .is_sorted());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str(":1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("2:4").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("5:").unwrap()),
        ])
        .is_sorted());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("1:2").unwrap()),
        ])
        .is_sorted());

        assert!(UserBoundsList(vec![
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("1").unwrap()),
            BoundOrFiller::Bound(UserBounds::from_str("2").unwrap()),
        ])
        .is_sorted());
    }
}
