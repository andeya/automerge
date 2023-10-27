use crate::error::AutomergeError;
use crate::marks::{MarkSet, MarkStateMachine};
use crate::op_set::OpSet;
use crate::op_tree::{OpTree, OpTreeNode};
use crate::query::{ListState, MarkMap, OpSetMetadata, QueryResult, TreeQuery};
use crate::types::{Clock, Key, ListEncoding, Op, OpIds};
use std::fmt::Debug;
use std::sync::Arc;

/// The Nth query walks the tree to find the n-th Node. It skips parts of the tree where it knows
/// that the nth node can not be in them
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Nth<'a> {
    idx: ListState,
    clock: Option<Clock>,
    marks: Option<MarkMap<'a>>,
    pub(crate) ops: Vec<&'a Op>,
    pub(crate) ops_pos: Vec<usize>,
}

impl<'a> Nth<'a> {
    pub(crate) fn new(target: usize, encoding: ListEncoding, clock: Option<Clock>) -> Self {
        Nth {
            idx: ListState::new(encoding, target + 1),
            clock,
            marks: None,
            ops: vec![],
            ops_pos: vec![],
        }
    }

    pub(crate) fn with_marks(mut self) -> Self {
        self.marks = Some(Default::default());
        self
    }

    pub(crate) fn marks(&self, meta: &OpSetMetadata) -> Option<Arc<MarkSet>> {
        let mut marks = MarkStateMachine::default();
        if let Some(m) = &self.marks {
            for (id, mark_data) in m.iter() {
                marks.mark_begin(*id, mark_data, meta);
            }
        }
        marks.current().cloned()
    }

    pub(crate) fn pred(&self, ops: &OpSet) -> OpIds {
        ops.m.sorted_opids(self.ops.iter().map(|o| o.id))
    }

    /// Get the key
    pub(crate) fn key(&self) -> Result<Key, AutomergeError> {
        // the query collects the ops so we can use that to get the key they all use
        if let Some(e) = self.ops.first().and_then(|op| op.elemid()) {
            Ok(Key::Seq(e))
        } else {
            Err(AutomergeError::InvalidIndex(
                self.idx.target().saturating_sub(1),
            ))
        }
    }

    pub(crate) fn index(&self) -> usize {
        self.idx.last_index()
    }

    pub(crate) fn pos(&self) -> usize {
        self.idx.pos()
    }
}

impl<'a> TreeQuery<'a> for Nth<'a> {
    fn equiv(&mut self, other: &Self) -> bool {
        self.index() == other.index() && self.key() == other.key()
    }

    fn can_shortcut_search(&mut self, tree: &'a OpTree) -> bool {
        if self.marks.is_some() {
            // we could cache marks data but we're not now
            return false;
        }
        if let Some(last) = &tree.last_insert {
            if last.index == self.idx.target().saturating_sub(1) {
                if let Some(op) = tree.internal.get(last.pos) {
                    self.idx.seek(last);
                    self.ops.push(op);
                    self.ops_pos.push(last.pos);
                    return true;
                }
            }
        }
        false
    }

    fn query_node(&mut self, child: &'a OpTreeNode, ops: &[Op]) -> QueryResult {
        self.idx.check_if_node_is_clean(child);
        if self.clock.is_none() {
            self.idx.process_node(child, ops, self.marks.as_mut())
        } else {
            QueryResult::Descend
        }
    }

    fn query_element(&mut self, element: &'a Op) -> QueryResult {
        if element.insert && self.idx.done() {
            QueryResult::Finish
        } else {
            if let Some(m) = self.marks.as_mut() {
                m.process(element)
            }
            let visible = element.visible_at(self.clock.as_ref());
            let key = element.elemid_or_key();
            self.idx.process_op(element, key, visible);
            if visible && self.idx.done() {
                self.ops.push(element);
                self.ops_pos.push(self.idx.pos().saturating_sub(1));
            }
            QueryResult::Next
        }
    }
}
