// Build the PSB v2+ key-names trie from a sorted list of keys.
//
// The trie stores names as a parent-pointer tree where each node's index in
// the `tree` array is computed so that:
//   child_index = value_offsets[parent] + character_byte
//
// At read time, walking up from a leaf and recording (current - value_offsets[parent])
// at each step recovers the bytes of the name.
//
// The packing algorithm is first-fit: for each parent node we find the lowest
// non-conflicting base offset such that all its children land in unused slots.

use std::collections::BTreeMap;

pub struct KeyNamesTrie {
    pub value_offsets: Vec<u64>,
    pub tree: Vec<u64>,
    pub tails: Vec<u64>,
}

struct Node {
    parent: usize,
    children: BTreeMap<u8, usize>,
    terminal: Option<usize>, // string index if this node terminates a name (char=0)
}

impl KeyNamesTrie {
    pub fn build(sorted_names: &[String]) -> Self {
        if sorted_names.is_empty() {
            return Self {
                value_offsets: vec![0],
                tree: vec![0],
                tails: Vec::new(),
            };
        }

        // Build raw trie nodes
        let mut nodes: Vec<Node> = vec![Node {
            parent: 0,
            children: BTreeMap::new(),
            terminal: None,
        }];

        for (str_idx, name) in sorted_names.iter().enumerate() {
            let mut curr = 0usize;
            for &ch in name.as_bytes() {
                let next = match nodes[curr].children.get(&ch) {
                    Some(&i) => i,
                    None => {
                        let new_idx = nodes.len();
                        nodes.push(Node {
                            parent: curr,
                            children: BTreeMap::new(),
                            terminal: None,
                        });
                        nodes[curr].children.insert(ch, new_idx);
                        new_idx
                    }
                };
                curr = next;
            }
            // Terminal node with char=0
            let term_idx = nodes.len();
            nodes.push(Node {
                parent: curr,
                children: BTreeMap::new(),
                terminal: Some(str_idx),
            });
            nodes[curr].children.insert(0, term_idx);
        }

        // Assign tree indices via first-fit allocation
        let mut used = vec![false];
        used[0] = true;
        let mut node_indices = vec![0usize; nodes.len()];
        let mut value_offs = vec![0u64];

        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        queue.push_back(0);

        while let Some(curr_idx) = queue.pop_front() {
            let chars: Vec<u8> = nodes[curr_idx].children.keys().copied().collect();
            if chars.is_empty() {
                continue;
            }
            let min_char = chars[0];

            // Find lowest base offset where all children fit
            let mut min_slot = std::cmp::max(1usize, min_char as usize + 1);
            loop {
                let mut ok = true;
                for &ch in &chars {
                    let target = min_slot + (ch - min_char) as usize;
                    if target < used.len() && used[target] {
                        ok = false;
                        break;
                    }
                }
                if ok {
                    break;
                }
                min_slot += 1;
            }

            // Grow arrays
            let max_needed = min_slot + (chars[chars.len() - 1] - min_char) as usize;
            while used.len() <= max_needed {
                used.push(false);
                value_offs.push(0);
            }

            // Assign indices
            for &ch in &chars {
                let child_idx = nodes[curr_idx].children[&ch];
                let assigned = min_slot + (ch - min_char) as usize;
                node_indices[child_idx] = assigned;
                used[assigned] = true;
                queue.push_back(child_idx);
            }

            value_offs[node_indices[curr_idx]] = (min_slot as i64 - min_char as i64) as u64;
        }

        // Materialize output arrays
        let n = used.len();
        let mut tree = vec![0u64; n];
        let mut value_offsets = vec![0u64; n];
        let mut tails = vec![0u64; sorted_names.len()];

        for (i, node) in nodes.iter().enumerate() {
            let idx = node_indices[i];
            let parent_idx = if i == 0 { 0 } else { node_indices[node.parent] };
            tree[idx] = parent_idx as u64;
            if let Some(term) = node.terminal {
                tails[term] = idx as u64;
                value_offsets[idx] = term as u64;
            } else {
                value_offsets[idx] = value_offs[idx];
            }
        }

        Self {
            value_offsets,
            tree,
            tails,
        }
    }
}
