//! Arbre à cliquet de MLS en tableau (RFC 9420, 4 et 7 ; annexe C) : les
//! feuilles sont aux indices pairs, l'arbre est toujours complet (2^k feuilles).

use super::super::Error;
use super::super::codec::{R, Reader, Writer, build};
use super::super::crypto;
use super::msg::LeafNode;

// ---------------------------------------------------------------- calculs d'indices

fn log2(x: usize) -> u32 {
    if x == 0 { 0 } else { usize::BITS - 1 - x.leading_zeros() }
}

pub fn level(x: usize) -> u32 {
    x.trailing_ones()
}

pub fn root(n_leaves: usize) -> usize {
    let w = 2 * n_leaves - 1;
    (1 << log2(w)) - 1
}

pub fn left(x: usize) -> usize {
    let k = level(x);
    x ^ (1 << (k - 1))
}

pub fn right(x: usize) -> usize {
    let k = level(x);
    x ^ (3 << (k - 1))
}

pub fn parent(x: usize) -> usize {
    let k = level(x);
    let b = (x >> (k + 1)) & 1;
    (x | (1 << k)) ^ (b << (k + 1))
}

#[cfg(test)]
pub fn sibling(x: usize) -> usize {
    let p = parent(x);
    if x < p { right(p) } else { left(p) }
}

pub fn leaf_node(i: u32) -> usize {
    2 * i as usize
}

/// Chemin direct : parents de `x` jusqu'à la racine comprise.
pub fn direct_path(x: usize, n_leaves: usize) -> Vec<usize> {
    let r = root(n_leaves);
    let mut out = Vec::new();
    let mut y = x;
    while y != r {
        y = parent(y);
        out.push(y);
    }
    out
}

/// `x` est-il dans le sous-arbre de racine `top` ?
pub fn in_subtree(x: usize, top: usize) -> bool {
    let k = level(top);
    let lo = top - ((1 << k) - 1);
    let hi = top + ((1 << k) - 1);
    x >= lo && x <= hi
}

/// Plus proche ancêtre commun de deux nœuds.
pub fn common_ancestor(a: usize, b: usize) -> usize {
    let (mut x, mut y) = (a, b);
    let mut k = 0;
    while x != y {
        x >>= 1;
        y >>= 1;
        k += 1;
    }
    (x << k) + (1 << (k - 1)) - 1
}

// ---------------------------------------------------------------- nœuds

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParentNode {
    pub enc: Vec<u8>,
    pub parent_hash: Vec<u8>,
    pub unmerged: Vec<u32>,
}

impl ParentNode {
    fn write(&self, w: &mut Writer) {
        w.vec(&self.enc).vec(&self.parent_hash).nested(|w| {
            for u in &self.unmerged {
                w.u32(*u);
            }
        });
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Node {
    Leaf(LeafNode),
    Parent(ParentNode),
}

impl Node {
    pub fn enc(&self) -> &[u8] {
        match self {
            Node::Leaf(l) => &l.enc,
            Node::Parent(p) => &p.enc,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Option<Node>>,
}

impl Tree {
    pub fn with_leaf(leaf: LeafNode) -> Self {
        Tree { nodes: vec![Some(Node::Leaf(leaf))] }
    }

    pub fn n_leaves(&self) -> usize {
        self.nodes.len().div_ceil(2)
    }

    pub fn leaf(&self, i: u32) -> Option<&LeafNode> {
        match self.nodes.get(leaf_node(i)) {
            Some(Some(Node::Leaf(l))) => Some(l),
            _ => None,
        }
    }

    pub fn parent_node(&self, x: usize) -> Option<&ParentNode> {
        match self.nodes.get(x) {
            Some(Some(Node::Parent(p))) => Some(p),
            _ => None,
        }
    }

    pub fn leaves(&self) -> impl Iterator<Item = (u32, &LeafNode)> {
        (0..self.n_leaves() as u32).filter_map(|i| self.leaf(i).map(|l| (i, l)))
    }

    /// Extension ratchet_tree : `optional<Node> ratchet_tree<V>`.
    pub fn read(data: &[u8]) -> R<Self> {
        let mut r = Reader::new(data);
        let mut list = r.sub()?;
        r.end()?;
        let mut nodes = Vec::new();
        while !list.is_empty() {
            if !list.present()? {
                nodes.push(None);
                continue;
            }
            let ty = list.u8()?;
            let is_leaf = nodes.len() % 2 == 0;
            let node = match ty {
                1 if is_leaf => Node::Leaf(LeafNode::read(&mut list)?),
                2 if !is_leaf => {
                    let enc = list.vec()?.to_vec();
                    let parent_hash = list.vec()?.to_vec();
                    let mut u = list.sub()?;
                    let mut unmerged = Vec::new();
                    while !u.is_empty() {
                        unmerged.push(u.u32()?);
                    }
                    Node::Parent(ParentNode { enc, parent_hash, unmerged })
                }
                _ => return Err(Error("arbre : type de nœud")),
            };
            nodes.push(Some(node));
        }
        if nodes.is_empty() || nodes.len() % 2 == 0 || !matches!(nodes.last(), Some(Some(Node::Leaf(_)))) {
            return Err(Error("arbre : forme invalide"));
        }
        if nodes.len() > 1 << 16 {
            return Err(Error("arbre trop grand"));
        }
        // Les nœuds vides en fin d'arbre sont omis : compléter jusqu'à 2^k feuilles.
        let n = nodes.len().div_ceil(2).next_power_of_two();
        nodes.resize(2 * n - 1, None);
        Ok(Tree { nodes })
    }

    pub fn write(&self) -> Vec<u8> {
        let mut end = self.nodes.len();
        while end > 0 && self.nodes[end - 1].is_none() {
            end -= 1;
        }
        build(|w| {
            w.nested(|w| {
                for n in &self.nodes[..end] {
                    match n {
                        None => {
                            w.u8(0);
                        }
                        Some(Node::Leaf(l)) => {
                            w.u8(1).u8(1);
                            l.write(w);
                        }
                        Some(Node::Parent(p)) => {
                            w.u8(1).u8(2);
                            p.write(w);
                        }
                    }
                }
            });
        })
    }

    // ------------------------------------------------------------ hachages

    pub fn tree_hash(&self) -> Vec<u8> {
        self.hash_at(root(self.n_leaves()), &[])
    }

    /// Hachage du sous-arbre en `x`, les feuilles `exclude` étant tenues
    /// pour vides (hachage « d'origine » du frère, RFC 9420, 7.9).
    pub fn hash_at(&self, x: usize, exclude: &[u32]) -> Vec<u8> {
        if level(x) == 0 {
            let i = (x / 2) as u32;
            let leaf = match &self.nodes[x] {
                Some(Node::Leaf(l)) if !exclude.contains(&i) => Some(l),
                _ => None,
            };
            return crypto::hash(&build(|w| {
                w.u8(1).u32(i);
                match leaf {
                    Some(l) => {
                        w.u8(1);
                        l.write(w);
                    }
                    None => {
                        w.u8(0);
                    }
                }
            }));
        }
        let lh = self.hash_at(left(x), exclude);
        let rh = self.hash_at(right(x), exclude);
        crypto::hash(&build(|w| {
            w.u8(2);
            match &self.nodes[x] {
                Some(Node::Parent(p)) => {
                    w.u8(1);
                    let p = ParentNode {
                        enc: p.enc.clone(),
                        parent_hash: p.parent_hash.clone(),
                        unmerged: p.unmerged.iter().copied().filter(|u| !exclude.contains(u)).collect(),
                    };
                    p.write(w);
                }
                _ => {
                    w.u8(0);
                }
            }
            w.vec(&lh).vec(&rh);
        }))
    }

    /// Hachage parent de `p` vu depuis son enfant `c` (le frère étant l'autre).
    pub fn parent_hash_of(&self, p: usize, c: usize) -> Option<Vec<u8>> {
        let node = self.parent_node(p)?;
        let s = if left(p) == c { right(p) } else { left(p) };
        let sib = self.hash_at(s, &node.unmerged);
        Some(crypto::hash(&build(|w| {
            w.vec(&node.enc).vec(&node.parent_hash).vec(&sib);
        })))
    }

    // ------------------------------------------------------------ résolution

    pub fn resolution(&self, x: usize) -> Vec<usize> {
        match &self.nodes[x] {
            Some(Node::Leaf(_)) => vec![x],
            Some(Node::Parent(p)) => {
                let mut v = vec![x];
                v.extend(p.unmerged.iter().map(|u| leaf_node(*u)));
                v
            }
            None if level(x) == 0 => Vec::new(),
            None => {
                let mut v = self.resolution(left(x));
                v.extend(self.resolution(right(x)));
                v
            }
        }
    }

    /// Chemin direct filtré de la feuille `i` : (nœud, enfant du côté opposé).
    pub fn filtered_direct_path(&self, i: u32) -> Vec<(usize, usize)> {
        let x = leaf_node(i);
        direct_path(x, self.n_leaves())
            .into_iter()
            .filter_map(|p| {
                let copath = if in_subtree(x, left(p)) { right(p) } else { left(p) };
                (!self.resolution(copath).is_empty()).then_some((p, copath))
            })
            .collect()
    }

    // ------------------------------------------------------------ modifications

    pub fn add_leaf(&mut self, leaf: LeafNode) -> u32 {
        let n = self.n_leaves();
        let i = (0..n as u32).find(|&i| self.nodes[leaf_node(i)].is_none()).unwrap_or_else(|| {
            // Plus de feuille libre : doubler l'arbre.
            let old = self.nodes.len();
            self.nodes.resize(2 * old + 1, None);
            n as u32
        });
        let x = leaf_node(i);
        self.nodes[x] = Some(Node::Leaf(leaf));
        for p in direct_path(x, self.n_leaves()) {
            if let Some(Some(Node::Parent(pn))) = self.nodes.get_mut(p) {
                pn.unmerged.push(i);
            }
        }
        i
    }

    pub fn remove_leaf(&mut self, i: u32) -> R<()> {
        let x = leaf_node(i);
        if !matches!(self.nodes.get(x), Some(Some(Node::Leaf(_)))) {
            return Err(Error("retrait d'une feuille vide"));
        }
        self.nodes[x] = None;
        for p in direct_path(x, self.n_leaves()) {
            self.nodes[p] = None;
        }
        // Tronquer tant que le sous-arbre droit de la racine est entièrement
        // vide (la racine part avec lui, vide ou non).
        while self.nodes.len() > 1 {
            let half = self.nodes.len() / 2;
            if self.nodes[half + 1..].iter().any(Option::is_some) {
                break;
            }
            self.nodes.truncate(half);
        }
        Ok(())
    }

    /// Vide le chemin direct d'une feuille (avant d'y poser un nouveau chemin).
    pub fn blank_path(&mut self, i: u32) {
        for p in direct_path(leaf_node(i), self.n_leaves()) {
            self.nodes[p] = None;
        }
    }

    // ------------------------------------------------------------ validation

    /// Validité des hachages parents de tout l'arbre (RFC 9420, 7.9.2).
    pub fn parent_hashes_valid(&self) -> bool {
        (0..self.nodes.len()).filter(|&x| level(x) > 0).all(|p| {
            let Some(node) = self.parent_node(p) else { return true };
            let mut found = 0;
            for c in [left(p), right(p)] {
                let Some(expected) = self.parent_hash_of(p, c) else { return false };
                let res = self.resolution(c);
                let mut unmerged_under: Vec<usize> = node
                    .unmerged
                    .iter()
                    .map(|u| leaf_node(*u))
                    .filter(|&l| in_subtree(l, c))
                    .collect();
                unmerged_under.sort_unstable();
                for &d in &res {
                    let ph = match &self.nodes[d] {
                        Some(Node::Leaf(l)) => l.parent_hash(),
                        Some(Node::Parent(pn)) => Some(pn.parent_hash.as_slice()),
                        None => None,
                    };
                    if ph != Some(expected.as_slice()) {
                        continue;
                    }
                    let mut rest: Vec<usize> = res.iter().copied().filter(|&y| y != d).collect();
                    rest.sort_unstable();
                    if rest == unmerged_under {
                        found += 1;
                    }
                }
            }
            found == 1
        })
    }

    /// Feuilles non fusionnées cohérentes et clés de chiffrement uniques.
    pub fn structure_valid(&self) -> bool {
        let mut keys: Vec<&[u8]> = self.nodes.iter().flatten().map(Node::enc).collect();
        let total = keys.len();
        keys.sort_unstable();
        keys.dedup();
        if keys.len() != total {
            return false;
        }
        (0..self.nodes.len()).filter(|&x| level(x) > 0).all(|p| {
            let Some(node) = self.parent_node(p) else { return true };
            node.unmerged.iter().all(|&u| {
                let l = leaf_node(u);
                l < self.nodes.len()
                    && self.nodes[l].is_some()
                    && in_subtree(l, p)
                    && direct_path(l, self.n_leaves())
                        .into_iter()
                        .take_while(|&q| q != p)
                        .all(|q| self.parent_node(q).is_none_or(|pn| pn.unmerged.contains(&u)))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculs_d_indices_vecteurs_mls() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../../../../../tests/fixtures/dave/mls-tree-math.json")).unwrap();
        for case in v.as_array().unwrap() {
            let n = case["n_leaves"].as_u64().unwrap() as usize;
            assert_eq!(root(n) as u64, case["root"].as_u64().unwrap());
            let w = case["n_nodes"].as_u64().unwrap() as usize;
            assert_eq!(2 * n - 1, w);
            for x in 0..w {
                let get = |k: &str| case[k][x].as_u64().map(|v| v as usize);
                if level(x) > 0 {
                    assert_eq!(Some(left(x)), get("left"));
                    assert_eq!(Some(right(x)), get("right"));
                }
                if x != root(n) {
                    assert_eq!(Some(parent(x)), get("parent"));
                    assert_eq!(Some(sibling(x)), get("sibling"));
                }
            }
        }
    }

    #[test]
    fn ancetre_commun() {
        assert_eq!(common_ancestor(0, 2), 1);
        assert_eq!(common_ancestor(0, 4), 3);
        assert_eq!(common_ancestor(8, 14), 11);
        assert!(in_subtree(4, 3) && !in_subtree(8, 3));
    }
}
