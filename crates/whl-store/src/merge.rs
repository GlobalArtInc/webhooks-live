use std::cmp::Ordering;

use crate::capture_queries::Order;
use crate::stream::CaptureSummary;

#[derive(Debug, Clone, PartialEq)]
pub struct MergedLanes {
    pub items: Vec<CaptureSummary>,
    pub taken: Vec<usize>,
}

fn rank(left: &CaptureSummary, right: &CaptureSummary) -> Ordering {
    left.received_at
        .cmp(&right.received_at)
        .then_with(|| left.id.cmp(&right.id))
}

pub fn merge_lanes(lanes: &[Vec<CaptureSummary>], order: Order, limit: usize) -> MergedLanes {
    let mut taken = vec![0_usize; lanes.len()];
    let mut items = Vec::with_capacity(limit.min(lanes.iter().map(Vec::len).sum()));
    while items.len() < limit {
        let mut best: Option<usize> = None;
        for (index, lane) in lanes.iter().enumerate() {
            let Some(head) = lane.get(taken[index]) else {
                continue;
            };
            let better = match best {
                None => true,
                Some(current) => {
                    let incumbent = &lanes[current][taken[current]];
                    match order {
                        Order::Desc => rank(head, incumbent) == Ordering::Greater,
                        Order::Asc => rank(head, incumbent) == Ordering::Less,
                    }
                }
            };
            if better {
                best = Some(index);
            }
        }
        let Some(index) = best else {
            break;
        };
        items.push(lanes[index][taken[index]].clone());
        taken[index] += 1;
    }
    MergedLanes { items, taken }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::CaptureKind;

    fn summary(endpoint: &str, seq: i64, received_at: &str, id: &str) -> CaptureSummary {
        CaptureSummary {
            id: id.to_owned(),
            seq,
            endpoint_id: endpoint.to_owned(),
            received_at: received_at.to_owned(),
            method: "POST".to_owned(),
            path: String::new(),
            query_string: String::new(),
            client_ip: "203.0.113.1".to_owned(),
            content_type: None,
            body_size: 0,
            status_override: None,
            response_status: 200,
            forward_state: None,
            kind: CaptureKind::Http,
        }
    }

    fn ids(merged: &MergedLanes) -> Vec<&str> {
        merged.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn interleaves_lanes_newest_first_and_counts_what_each_lane_gave() {
        let first = vec![
            summary("a", 3, "2026-01-01T00:00:03.000Z", "a3"),
            summary("a", 1, "2026-01-01T00:00:01.000Z", "a1"),
        ];
        let second = vec![summary("b", 2, "2026-01-01T00:00:02.000Z", "b2")];
        let merged = merge_lanes(&[first, second], Order::Desc, 2);
        assert_eq!(ids(&merged), ["a3", "b2"]);
        assert_eq!(merged.taken, [1, 1]);
    }

    #[test]
    fn equal_timestamps_fall_back_to_the_id_so_the_order_is_total() {
        let first = vec![summary("a", 1, "2026-01-01T00:00:01.000Z", "x1")];
        let second = vec![summary("b", 1, "2026-01-01T00:00:01.000Z", "x2")];
        let newest = merge_lanes(&[first.clone(), second.clone()], Order::Desc, 2);
        let oldest = merge_lanes(&[first, second], Order::Asc, 2);
        assert_eq!(ids(&newest), ["x2", "x1"]);
        assert_eq!(ids(&oldest), ["x1", "x2"]);
    }

    #[test]
    fn a_lane_is_never_reordered_even_when_its_clock_ran_backwards() {
        let skewed = vec![
            summary("a", 2, "2026-01-01T00:00:01.000Z", "a2"),
            summary("a", 1, "2026-01-01T00:00:05.000Z", "a1"),
        ];
        let other = vec![summary("b", 1, "2026-01-01T00:00:03.000Z", "b1")];
        let merged = merge_lanes(&[skewed, other], Order::Desc, 3);
        let order = ids(&merged);
        let first = order.iter().position(|id| *id == "a2").expect("present");
        let second = order.iter().position(|id| *id == "a1").expect("present");
        assert!(first < second);
        assert_eq!(order.len(), 3);
    }
}
