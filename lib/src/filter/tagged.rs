use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use globset::GlobMatcher;
use regex::Regex;
use tracing::{debug, trace, warn};

use crate::error::RuntimeError;
use crate::event::{Event, Tag};
use crate::filter::Filterer;

mod parse;
pub mod swaplock;

pub struct TaggedFilterer {
	/// The directory the project is in, its "root".
	///
	/// This is used to resolve absolute paths without an `in_path` context.
	root: PathBuf,

	/// Where the program is running from.
	///
	/// This is used to resolve relative paths without an `in_path` context.
	workdir: PathBuf,

	/// All filters that are applied, in order, by matcher.
	filters: swaplock::SwapLock<HashMap<Matcher, Vec<Filter>>>,
}

impl Filterer for TaggedFilterer {
	fn check_event(&self, event: &Event) -> Result<bool, RuntimeError> {
		// TODO: trace logging
		if self.filters.borrow().is_empty() {
			trace!("no filters, skipping entire check (pass)");
			return Ok(true);
		}

		trace!(tags=%event.tags.len(), "checking all tags on the event");
		for tag in &event.tags {
			let filters = self.filters.borrow().get(&tag.into()).cloned();
			if let Some(tag_filters) = filters {
				trace!(?tag, "checking tag");

				if tag_filters.is_empty() {
					trace!(?tag, "no filters for this tag, skipping (pass)");
					continue;
				}

				trace!(?tag, filters=%tag_filters.len(), "found some filters for this tag");

				let mut tag_match = true;
				for filter in &tag_filters {
					trace!(?filter, ?tag, "checking filter againt tag");
					if let Some(app) = self.match_tag(filter, tag)? {
						if filter.negate {
							if app {
								trace!(prev=%tag_match, now=%true, "negate filter passes, resetting tag to pass");
								tag_match = true;
							} else {
								trace!(prev=%tag_match, now=%tag_match, "negate filter fails, ignoring");
							}
						} else {
							trace!(prev=%tag_match, this=%app, now=%(tag_match&app), "filter applies to this tag");
							tag_match &= app;
						}
					}
				}

				if !tag_match {
					trace!(?tag, "tag fails check, failing entire event");
					return Ok(false);
				}

				trace!(?tag, "tag passes check, continuing");
			} else {
				trace!(?tag, "no filters for this tag, skipping (pass)");
			}
		}

		trace!(?event, "passing event");
		Ok(true)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
	/// Path the filter applies from.
	pub in_path: Option<PathBuf>,

	/// Which tag the filter applies to.
	pub on: Matcher,

	/// The operation to perform on the tag's value.
	pub op: Op,

	/// The pattern to match against the tag's value.
	pub pat: Pattern,

	/// If true, a positive match with this filter will override negative matches from previous
	/// filters on the same tag, and negative matches will be ignored.
	pub negate: bool,
}

impl TaggedFilterer {
	pub fn new(root: impl Into<PathBuf>, workdir: impl Into<PathBuf>) -> Arc<Self> {
		Arc::new(Self {
			root: root.into(),
			workdir: workdir.into(),
			filters: swaplock::SwapLock::new(HashMap::new()),
		})
	}

	fn match_tag(&self, filter: &Filter, tag: &Tag) -> Result<Option<bool>, RuntimeError> {
		trace!(?tag, matcher=?filter.on, "matching filter to tag");
		match (tag, filter.on) {
			(tag, Matcher::Tag) => filter.matches(tag.discriminant_name()),
			(Tag::Path(_path), Matcher::Path) => todo!("tagged filterer: path matcher"),
			(Tag::FileEventKind(kind), Matcher::FileEventKind) => {
				filter.matches(format!("{:?}", kind))
			}
			(Tag::Source(src), Matcher::Source) => filter.matches(src.to_string()),
			(Tag::Process(pid), Matcher::Process) => filter.matches(pid.to_string()),
			(Tag::Signal(_sig), Matcher::Signal) => todo!("tagged filterer: signal matcher"),
			(Tag::ProcessCompletion(_oes), Matcher::ProcessCompletion) => {
				todo!("tagged filterer: completion matcher")
			}
			(tag, matcher) => {
				trace!(?tag, ?matcher, "no match for tag, skipping");
				return Ok(None);
			}
		}
		.map(Some)
	}

	pub async fn add_filter(&self, filter: Filter) -> Result<(), RuntimeError> {
		debug!(?filter, "adding filter to filterer");
		self.filters
			.change(|filters| {
				filters.entry(filter.on).or_default().push(filter);
			})
			.await
			.map_err(|err| RuntimeError::FilterChange { action: "add", err })?;
		Ok(())
	}

	pub async fn remove_filter(&self, filter: &Filter) -> Result<(), RuntimeError> {
		debug!(?filter, "removing filter from filterer");
		self.filters
			.change(|filters| {
				filters
					.entry(filter.on)
					.or_default()
					.retain(|f| f != filter);
			})
			.await
			.map_err(|err| RuntimeError::FilterChange {
				action: "remove",
				err,
			})?;
		Ok(())
	}

	pub async fn clear_filters(&self) -> Result<(), RuntimeError> {
		debug!("removing all filters from filterer");
		self.filters
			.replace(Default::default())
			.await
			.map_err(|err| RuntimeError::FilterChange {
				action: "clear all",
				err,
			})?;
		Ok(())
	}
}

impl Filter {
	pub fn matches(&self, subject: impl AsRef<str>) -> Result<bool, RuntimeError> {
		let subject = subject.as_ref();

		trace!(op=?self.op, pat=?self.pat, ?subject, "performing filter match");
		Ok(match (self.op, &self.pat) {
			(Op::Equal, Pattern::Exact(pat)) => subject == pat,
			(Op::NotEqual, Pattern::Exact(pat)) => subject != pat,
			(Op::Regex, Pattern::Regex(pat)) => pat.is_match(subject),
			(Op::NotRegex, Pattern::Regex(pat)) => !pat.is_match(subject),
			(Op::Glob, Pattern::Glob(pat)) => pat.is_match(subject),
			(Op::NotGlob, Pattern::Glob(pat)) => !pat.is_match(subject),
			(Op::InSet, Pattern::Set(set)) => set.contains(subject),
			(Op::InSet, Pattern::Exact(pat)) => subject == pat,
			(Op::NotInSet, Pattern::Set(set)) => !set.contains(subject),
			(Op::NotInSet, Pattern::Exact(pat)) => subject != pat,
			(op, pat) => {
				warn!(
					"trying to match pattern {:?} with op {:?}, that cannot work",
					pat, op
				);
				false
			}
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Matcher {
	Tag,
	Path,
	FileEventKind,
	Source,
	Process,
	Signal,
	ProcessCompletion,
}

impl From<&Tag> for Matcher {
	fn from(tag: &Tag) -> Self {
		match tag {
			Tag::Path(_) => Matcher::Path,
			Tag::FileEventKind(_) => Matcher::FileEventKind,
			Tag::Source(_) => Matcher::Source,
			Tag::Process(_) => Matcher::Process,
			Tag::Signal(_) => Matcher::Signal,
			Tag::ProcessCompletion(_) => Matcher::ProcessCompletion,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
	Auto,     // =
	Equal,    // ==
	NotEqual, // !=
	Regex,    // ~=
	NotRegex, // ~!
	Glob,     // *=
	NotGlob,  // *!
	InSet,    // :=
	NotInSet, // :!
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Pattern {
	Exact(String),
	Regex(Regex),
	Glob(GlobMatcher),
	Set(HashSet<String>),
}

impl PartialEq<Self> for Pattern {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			(Self::Exact(l), Self::Exact(r)) => l == r,
			(Self::Regex(l), Self::Regex(r)) => l.as_str() == r.as_str(),
			(Self::Glob(l), Self::Glob(r)) => l.glob() == r.glob(),
			(Self::Set(l), Self::Set(r)) => l == r,
			_ => false,
		}
	}
}

impl Eq for Pattern {}