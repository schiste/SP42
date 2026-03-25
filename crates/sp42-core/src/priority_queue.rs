//! Deterministic priority queue for patrol work items.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Debug)]
pub struct QueueEntry<T> {
    pub priority: i32,
    pub sequence: u64,
    pub value: T,
}

impl<T> PartialEq for QueueEntry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.sequence == other.sequence
    }
}

impl<T> Eq for QueueEntry<T> {}

impl<T> PartialOrd for QueueEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for QueueEntry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.priority
            .cmp(&other.priority)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

#[derive(Debug)]
pub struct PriorityQueue<T> {
    heap: BinaryHeap<QueueEntry<T>>,
    next_sequence: u64,
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            heap: BinaryHeap::new(),
            next_sequence: 0,
        }
    }
}

impl<T> PriorityQueue<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, priority: i32, value: T) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.heap.push(QueueEntry {
            priority,
            sequence,
            value,
        });
        sequence
    }

    pub fn pop(&mut self) -> Option<T> {
        self.heap.pop().map(|entry| entry.value)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::PriorityQueue;
    use proptest::prelude::*;

    #[test]
    fn dequeues_highest_priority_first() {
        let mut queue = PriorityQueue::new();
        queue.push(10, "low");
        queue.push(20, "high");

        assert_eq!(queue.pop(), Some("high"));
        assert_eq!(queue.pop(), Some("low"));
    }

    #[test]
    fn preserves_insertion_order_for_equal_priorities() {
        let mut queue = PriorityQueue::new();
        queue.push(10, "first");
        queue.push(10, "second");

        assert_eq!(queue.pop(), Some("first"));
        assert_eq!(queue.pop(), Some("second"));
    }

    proptest! {
        #[test]
        fn property_dequeues_in_non_increasing_priority_order(priorities in prop::collection::vec(-500i32..500, 1..64)) {
            let mut queue = PriorityQueue::new();
            for priority in &priorities {
                queue.push(*priority, *priority);
            }

            let mut popped = Vec::new();
            while let Some(value) = queue.pop() {
                popped.push(value);
            }

            let mut expected = priorities.clone();
            expected.sort_by(|left, right| right.cmp(left));

            prop_assert_eq!(popped, expected);
        }
    }
}
