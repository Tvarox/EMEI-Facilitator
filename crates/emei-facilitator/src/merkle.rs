use alloy_primitives::keccak256;

pub struct MerkleTree {
    leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    pub fn new(mut leaves: Vec<[u8; 32]>) -> Self {
        leaves.sort();
        Self { leaves }
    }

    pub fn root(&self) -> [u8; 32] {
        if self.leaves.is_empty() {
            return [0u8; 32];
        }
        if self.leaves.len() == 1 {
            return self.leaves[0];
        }
        compute_root(&self.leaves)
    }

    pub fn proof(&self, leaf: &[u8; 32]) -> Option<Vec<[u8; 32]>> {
        let index = self.leaves.iter().position(|l| l == leaf)?;
        Some(generate_proof(&self.leaves, index))
    }

    pub fn verify(root: &[u8; 32], leaf: &[u8; 32], proof: &[[u8; 32]]) -> bool {
        let mut computed = *leaf;
        for sibling in proof {
            computed = hash_pair(computed, *sibling);
        }
        &computed == root
    }
}

fn hash_pair(a: [u8; 32], b: [u8; 32]) -> [u8; 32] {
    let (left, right) = if a <= b { (a, b) } else { (b, a) };
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(&left);
    combined[32..].copy_from_slice(&right);
    keccak256(combined).into()
}

fn compute_root(layer: &[[u8; 32]]) -> [u8; 32] {
    if layer.len() == 1 {
        return layer[0];
    }
    let mut next_layer = Vec::with_capacity((layer.len() + 1) / 2);
    for chunk in layer.chunks(2) {
        if chunk.len() == 2 {
            next_layer.push(hash_pair(chunk[0], chunk[1]));
        } else {
            // Odd element is promoted to the next layer
            next_layer.push(chunk[0]);
        }
    }
    compute_root(&next_layer)
}

fn generate_proof(leaves: &[[u8; 32]], index: usize) -> Vec<[u8; 32]> {
    if leaves.len() <= 1 {
        return vec![];
    }

    let mut proof = Vec::new();
    let mut current_layer: Vec<[u8; 32]> = leaves.to_vec();
    let mut idx = index;

    while current_layer.len() > 1 {
        // Determine sibling index
        let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };

        // If sibling exists, add it to the proof
        if sibling_idx < current_layer.len() {
            proof.push(current_layer[sibling_idx]);
        }

        // Build next layer
        let mut next_layer = Vec::with_capacity((current_layer.len() + 1) / 2);
        for chunk in current_layer.chunks(2) {
            if chunk.len() == 2 {
                next_layer.push(hash_pair(chunk[0], chunk[1]));
            } else {
                next_layer.push(chunk[0]);
            }
        }

        // Move index to parent position
        idx /= 2;
        current_layer = next_layer;
    }

    proof
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_tree_returns_zero_root() {
        let tree = MerkleTree::new(vec![]);
        assert_eq!(tree.root(), [0u8; 32]);
    }

    #[test]
    fn single_leaf_is_root() {
        let leaf = [42u8; 32];
        let tree = MerkleTree::new(vec![leaf]);
        assert_eq!(tree.root(), leaf);
    }

    #[test]
    fn two_leaves_correct_root() {
        let leaf_a = [1u8; 32];
        let leaf_b = [2u8; 32];
        let tree = MerkleTree::new(vec![leaf_a, leaf_b]);

        // Since leaves are sorted and leaf_a < leaf_b, root = hash_pair(leaf_a, leaf_b)
        let expected_root = hash_pair(leaf_a, leaf_b);
        assert_eq!(tree.root(), expected_root);
    }

    #[test]
    fn two_leaves_order_independent() {
        let leaf_a = [1u8; 32];
        let leaf_b = [2u8; 32];

        let tree1 = MerkleTree::new(vec![leaf_a, leaf_b]);
        let tree2 = MerkleTree::new(vec![leaf_b, leaf_a]);

        // Both orderings should produce the same root (leaves are sorted)
        assert_eq!(tree1.root(), tree2.root());
    }

    #[test]
    fn proof_verification_two_leaves() {
        let leaf_a = [1u8; 32];
        let leaf_b = [2u8; 32];
        let tree = MerkleTree::new(vec![leaf_a, leaf_b]);
        let root = tree.root();

        let proof_a = tree.proof(&leaf_a).expect("leaf_a should be in tree");
        assert!(MerkleTree::verify(&root, &leaf_a, &proof_a));

        let proof_b = tree.proof(&leaf_b).expect("leaf_b should be in tree");
        assert!(MerkleTree::verify(&root, &leaf_b, &proof_b));
    }

    #[test]
    fn proof_verification_four_leaves() {
        let leaves: Vec<[u8; 32]> = (1..=4u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves.clone());
        let root = tree.root();

        for leaf in &leaves {
            let proof = tree.proof(leaf).expect("leaf should be in tree");
            assert!(
                MerkleTree::verify(&root, leaf, &proof),
                "verification failed for leaf {:?}",
                leaf[0]
            );
        }
    }

    #[test]
    fn proof_verification_eight_leaves() {
        let leaves: Vec<[u8; 32]> = (1..=8u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves.clone());
        let root = tree.root();

        for leaf in &leaves {
            let proof = tree.proof(leaf).expect("leaf should be in tree");
            assert!(
                MerkleTree::verify(&root, leaf, &proof),
                "verification failed for leaf {:?}",
                leaf[0]
            );
        }
    }

    #[test]
    fn proof_verification_odd_number_of_leaves() {
        let leaves: Vec<[u8; 32]> = (1..=5u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves.clone());
        let root = tree.root();

        for leaf in &leaves {
            let proof = tree.proof(leaf).expect("leaf should be in tree");
            assert!(
                MerkleTree::verify(&root, leaf, &proof),
                "verification failed for leaf {:?}",
                leaf[0]
            );
        }
    }

    #[test]
    fn proof_returns_none_for_missing_leaf() {
        let leaves: Vec<[u8; 32]> = (1..=4u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves);

        let missing = [99u8; 32];
        assert!(tree.proof(&missing).is_none());
    }

    #[test]
    fn verify_rejects_invalid_proof() {
        let leaves: Vec<[u8; 32]> = (1..=4u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves.clone());
        let root = tree.root();

        // Use a fake proof
        let fake_proof = vec![[0u8; 32], [0u8; 32]];
        assert!(!MerkleTree::verify(&root, &leaves[0], &fake_proof));
    }

    #[test]
    fn verify_rejects_wrong_root() {
        let leaves: Vec<[u8; 32]> = (1..=4u8).map(|i| [i; 32]).collect();
        let tree = MerkleTree::new(leaves.clone());

        let proof = tree.proof(&leaves[0]).expect("leaf should be in tree");
        let wrong_root = [0u8; 32];
        assert!(!MerkleTree::verify(&wrong_root, &leaves[0], &proof));
    }
}
