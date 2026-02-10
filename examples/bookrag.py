# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "mcp[cli]>=1.7",
#   "fastembed>=0.4",
#   "numpy>=1.26",
# ]
# ///
"""
BookRAG MCP Server — multi-book semantic search powered by boko section trees.

Implements the BookRAG architecture (arxiv 2512.03413): hierarchical section
tree + entity knowledge graph + GT-Link mapping with collapsed tree retrieval.

Designed as a Claude Code MCP skill: Claude Code itself generates summaries
via the get_sections_to_summarize / submit_summaries tool loop — no API key needed.

Usage:
    # Register as Claude Code skill (one-time)
    claude mcp add bookrag --scope user -- \\
        uv run /path/to/bookrag.py serve

    # Within Claude Code: ask it to index and query any epub

    # Standalone CLI indexing (no LLM summaries)
    boko sections book.epub | uv run examples/bookrag.py index --name mybook
"""

from __future__ import annotations

import argparse
import asyncio
import json
import math
import os
import shutil
import subprocess
import sys
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import numpy as np
from numpy.typing import NDArray

# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------

INDEX_DIR = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share")) / "bookrag"


@dataclass
class IndexNode:
    id: str
    kind: str  # "section" | "paragraph" | "code_block" | "block_quote" | "list" | "table" | "image"
    level: int
    title: str
    text: str  # raw content for leaves, summary for sections
    parent_id: str | None = None
    children: list[str] = field(default_factory=list)
    entities: list[str] = field(default_factory=list)  # entity names
    embedding_idx: int | None = None


@dataclass
class Entity:
    name: str
    description: str
    node_ids: list[str] = field(default_factory=list)


@dataclass
class Relation:
    source: str      # entity key (lowercase)
    target: str      # entity key (lowercase)
    rel_type: str    # e.g. "teaches", "opposes", "part_of"
    node_id: str     # tree node where this relation was extracted


@dataclass
class BookIndex:
    title: str
    authors: list[str]
    nodes: dict[str, IndexNode] = field(default_factory=dict)
    entities: dict[str, Entity] = field(default_factory=dict)
    relations: list[Relation] = field(default_factory=list)
    relations_extracted_node_ids: set[str] = field(default_factory=set)
    root_ids: list[str] = field(default_factory=list)
    embeddings: NDArray[np.float32] | None = None
    entity_embeddings: NDArray[np.float32] | None = None


# ---------------------------------------------------------------------------
# Serialization
# ---------------------------------------------------------------------------


def _node_to_dict(n: IndexNode) -> dict[str, Any]:
    return {
        "id": n.id,
        "kind": n.kind,
        "level": n.level,
        "title": n.title,
        "text": n.text,
        "parent_id": n.parent_id,
        "children": n.children,
        "entities": n.entities,
        "embedding_idx": n.embedding_idx,
    }


def _node_from_dict(d: dict[str, Any]) -> IndexNode:
    return IndexNode(
        id=d["id"],
        kind=d["kind"],
        level=d["level"],
        title=d["title"],
        text=d["text"],
        parent_id=d.get("parent_id"),
        children=d.get("children", []),
        entities=d.get("entities", []),
        embedding_idx=d.get("embedding_idx"),
    )


def _entity_to_dict(e: Entity) -> dict[str, Any]:
    return {"name": e.name, "description": e.description, "node_ids": e.node_ids}


def _entity_from_dict(d: dict[str, Any]) -> Entity:
    return Entity(name=d["name"], description=d["description"], node_ids=d.get("node_ids", []))


def _relation_to_dict(r: Relation) -> dict[str, Any]:
    return {"source": r.source, "target": r.target, "rel_type": r.rel_type, "node_id": r.node_id}


def _relation_from_dict(d: dict[str, Any]) -> Relation:
    return Relation(source=d["source"], target=d["target"], rel_type=d["rel_type"], node_id=d["node_id"])


def save_index(index: BookIndex, name: str) -> Path:
    out = INDEX_DIR / name
    out.mkdir(parents=True, exist_ok=True)
    data = {
        "title": index.title,
        "authors": index.authors,
        "root_ids": index.root_ids,
        "nodes": {k: _node_to_dict(v) for k, v in index.nodes.items()},
        "entities": {k: _entity_to_dict(v) for k, v in index.entities.items()},
        "relations": [_relation_to_dict(r) for r in index.relations],
        "relations_extracted_node_ids": sorted(index.relations_extracted_node_ids),
    }
    (out / "index.json").write_text(json.dumps(data, indent=2))
    if index.embeddings is not None:
        np.save(out / "embeddings.npy", index.embeddings)
    if index.entity_embeddings is not None:
        np.save(out / "entity_embeddings.npy", index.entity_embeddings)
    return out


def load_index(name: str) -> BookIndex:
    d = INDEX_DIR / name
    if not d.exists():
        raise FileNotFoundError(f"No index at {d}")
    data = json.loads((d / "index.json").read_text())
    idx = BookIndex(
        title=data["title"],
        authors=data["authors"],
        root_ids=data["root_ids"],
        nodes={k: _node_from_dict(v) for k, v in data["nodes"].items()},
        entities={k: _entity_from_dict(v) for k, v in data["entities"].items()},
        relations=[_relation_from_dict(r) for r in data.get("relations", [])],
        relations_extracted_node_ids=set(data.get("relations_extracted_node_ids", [])),
    )
    emb_path = d / "embeddings.npy"
    if emb_path.exists():
        idx.embeddings = np.load(emb_path)
    ent_emb_path = d / "entity_embeddings.npy"
    if ent_emb_path.exists():
        idx.entity_embeddings = np.load(ent_emb_path)
    return idx


def load_all_indexes() -> dict[str, BookIndex]:
    """Load all existing indexes from $XDG_DATA_HOME/bookrag/*/index.json."""
    books: dict[str, BookIndex] = {}
    if not INDEX_DIR.exists():
        return books
    for d in sorted(INDEX_DIR.iterdir()):
        if d.is_dir() and (d / "index.json").exists():
            try:
                books[d.name] = load_index(d.name)
            except Exception as e:
                print(f"Warning: failed to load index {d.name}: {e}", file=sys.stderr)
    return books


# ---------------------------------------------------------------------------
# Step 1: Parse boko sections JSON
# ---------------------------------------------------------------------------


def _content_block_text(block: dict[str, Any]) -> str:
    """Extract readable text from a ContentBlock."""
    t = block["type"]
    if t == "paragraph":
        return block["text"]
    if t == "code_block":
        lang = block.get("language") or ""
        return f"```{lang}\n{block['code']}\n```"
    if t == "block_quote":
        return f"> {block['text']}"
    if t == "list":
        items = block["items"]
        if block.get("ordered"):
            return "\n".join(f"{i + 1}. {item}" for i, item in enumerate(items))
        return "\n".join(f"- {item}" for item in items)
    if t == "table":
        rows = []
        headers = block.get("headers", [])
        if headers:
            rows.append("| " + " | ".join(headers) + " |")
            rows.append("| " + " | ".join("---" for _ in headers) + " |")
        for row in block.get("rows", []):
            rows.append("| " + " | ".join(row) + " |")
        return "\n".join(rows)
    if t == "image":
        return f"![{block.get('alt', '')}]({block.get('src', '')})"
    if t == "rule":
        return "---"
    return ""


def _content_block_kind(block: dict[str, Any]) -> str:
    return block["type"]


def parse_sections(tree: dict[str, Any]) -> BookIndex:
    """Walk boko section tree JSON, create IndexNodes."""
    index = BookIndex(
        title=tree.get("title", "Unknown"),
        authors=tree.get("authors", []),
    )

    def _make_leaf(block: dict[str, Any], level: int, parent_id: str | None) -> str | None:
        text = _content_block_text(block)
        if not text or len(text.strip()) < 10:
            return None
        kind = _content_block_kind(block)
        if kind == "rule" or kind == "image":
            return None
        nid = str(uuid.uuid4())
        node = IndexNode(
            id=nid,
            kind=kind,
            level=level + 1,
            title="",
            text=text,
            parent_id=parent_id,
        )
        index.nodes[nid] = node
        return nid

    def _walk_section(section: dict[str, Any], parent_id: str | None) -> str:
        nid = str(uuid.uuid4())
        level = section.get("level", 1)
        title = section.get("title", "")
        node = IndexNode(
            id=nid,
            kind="section",
            level=level,
            title=title,
            text="",  # filled by summarization
            parent_id=parent_id,
        )
        index.nodes[nid] = node

        for block in section.get("content", []):
            leaf_id = _make_leaf(block, level, nid)
            if leaf_id:
                node.children.append(leaf_id)

        for child in section.get("children", []):
            child_id = _walk_section(child, nid)
            node.children.append(child_id)

        return nid

    # Handle preamble
    preamble = tree.get("preamble", [])
    if preamble:
        pre_id = str(uuid.uuid4())
        pre_node = IndexNode(
            id=pre_id, kind="section", level=0, title="Preamble", text="", parent_id=None
        )
        index.nodes[pre_id] = pre_node
        for block in preamble:
            leaf_id = _make_leaf(block, 0, pre_id)
            if leaf_id:
                pre_node.children.append(leaf_id)
        if pre_node.children:
            index.root_ids.append(pre_id)
        else:
            del index.nodes[pre_id]

    for section in tree.get("sections", []):
        rid = _walk_section(section, None)
        index.root_ids.append(rid)

    return index


# ---------------------------------------------------------------------------
# Embedding with fastembed
# ---------------------------------------------------------------------------


_embed_model: TextEmbedding | None = None


def _get_embed_model() -> TextEmbedding:
    global _embed_model
    if _embed_model is None:
        from fastembed import TextEmbedding
        _embed_model = TextEmbedding("BAAI/bge-small-en-v1.5")
    return _embed_model


_rerank_model: TextCrossEncoder | None = None


def _get_rerank_model() -> TextCrossEncoder:
    global _rerank_model
    if _rerank_model is None:
        from fastembed.rerank.cross_encoder import TextCrossEncoder
        _rerank_model = TextCrossEncoder("Xenova/ms-marco-MiniLM-L-6-v2")
    return _rerank_model


def embed_texts(texts: list[str], prefix: str = "passage: ") -> NDArray[np.float32]:
    """Embed texts using fastembed (BAAI/bge-small-en-v1.5, 384d)."""
    model = _get_embed_model()
    prefixed = [prefix + t for t in texts]
    vecs = list(model.embed(prefixed))
    return np.array(vecs, dtype=np.float32)


def embed_leaves(index: BookIndex) -> None:
    """Embed all leaf nodes (non-section)."""
    leaves = [(nid, n) for nid, n in index.nodes.items() if n.kind != "section"]
    if not leaves:
        return
    texts = [n.text[:2000] for _, n in leaves]
    vecs = embed_texts(texts, prefix="passage: ")
    index.embeddings = vecs
    for i, (nid, _) in enumerate(leaves):
        index.nodes[nid].embedding_idx = i


def embed_summaries(index: BookIndex) -> None:
    """Embed section summaries, append to existing embeddings."""
    sections = [(nid, n) for nid, n in index.nodes.items() if n.kind == "section" and n.text]
    if not sections:
        return
    texts = [n.text[:2000] for _, n in sections]
    vecs = embed_texts(texts, prefix="passage: ")
    if index.embeddings is not None:
        offset = len(index.embeddings)
        index.embeddings = np.vstack([index.embeddings, vecs])
    else:
        offset = 0
        index.embeddings = vecs
    for i, (nid, _) in enumerate(sections):
        index.nodes[nid].embedding_idx = offset + i


# ---------------------------------------------------------------------------
# Summarization helpers (for CLI index --no-llm fallback)
# ---------------------------------------------------------------------------


def _gather_leaf_content(index: BookIndex, node: IndexNode) -> str:
    """Collect all leaf text under a section."""
    parts = []
    for cid in node.children:
        child = index.nodes[cid]
        if child.kind == "section":
            if child.text:
                parts.append(f"[{child.title}]: {child.text}")
        else:
            parts.append(child.text)
    return "\n\n".join(parts)


def _local_summarize(title: str, content: str) -> str:
    """Simple extractive summary: first ~300 chars of content."""
    text = content.strip().replace("\n", " ")
    if len(text) > 300:
        text = text[:300] + "..."
    return text


def summarize_local(index: BookIndex) -> None:
    """Local summarization fallback (no API key needed)."""
    sections_by_level: dict[int, list[IndexNode]] = {}
    for n in index.nodes.values():
        if n.kind == "section":
            sections_by_level.setdefault(n.level, []).append(n)

    for level in sorted(sections_by_level.keys(), reverse=True):
        for node in sections_by_level[level]:
            content = _gather_leaf_content(index, node)
            if not content.strip():
                node.text = f"Section: {node.title}"
                continue
            node.text = _local_summarize(node.title, content)
        print(f"  Summarized {len(sections_by_level[level])} sections at level {level}")


# ---------------------------------------------------------------------------
# Entity index
# ---------------------------------------------------------------------------


def _merge_entity_into(index: BookIndex, canonical: str, alias: str) -> None:
    """Merge alias entity into canonical, updating relations too."""
    canon_ent = index.entities[canonical]
    alias_ent = index.entities[alias]
    for nid in alias_ent.node_ids:
        if nid not in canon_ent.node_ids:
            canon_ent.node_ids.append(nid)
    if not canon_ent.description and alias_ent.description:
        canon_ent.description = alias_ent.description
    # Update relations to point to canonical key
    for rel in index.relations:
        if rel.source == alias:
            rel.source = canonical
        if rel.target == alias:
            rel.target = canonical
    del index.entities[alias]


def build_entity_index(index: BookIndex) -> None:
    """Build entity map with GT-Link propagation and embedding-based ER."""
    # Collect all entity mentions from nodes
    for node in index.nodes.values():
        for ename in node.entities:
            key = ename.lower().strip()
            if not key:
                continue
            if key not in index.entities:
                index.entities[key] = Entity(name=ename, description="", node_ids=[])
            ent = index.entities[key]
            if node.id not in ent.node_ids:
                ent.node_ids.append(node.id)

    # Propagate entities upward through parent chain
    for node in index.nodes.values():
        if not node.entities or not node.parent_id:
            continue
        current_id = node.parent_id
        while current_id:
            parent = index.nodes.get(current_id)
            if not parent:
                break
            for ename in node.entities:
                key = ename.lower().strip()
                if key in index.entities:
                    ent = index.entities[key]
                    if current_id not in ent.node_ids:
                        ent.node_ids.append(current_id)
            current_id = parent.parent_id

    # Embedding-based entity resolution (gradient algorithm from BookRAG paper)
    keys = list(index.entities.keys())
    if len(keys) < 2:
        return

    # Embed "name: description" strings for each entity
    texts = []
    for k in keys:
        ent = index.entities[k]
        text = ent.name
        if ent.description:
            text += ": " + ent.description
        texts.append(text)

    vecs = embed_texts(texts, prefix="passage: ")
    norms = np.linalg.norm(vecs, axis=1, keepdims=True)
    norms = np.where(norms == 0, 1, norms)
    normed = vecs / norms

    # Biencoder similarity for initial candidate retrieval
    sim_matrix = normed @ normed.T
    np.fill_diagonal(sim_matrix, 0.0)

    # Two-stage ER: biencoder retrieval → cross-encoder reranking → gradient detection
    reranker = _get_rerank_model()
    g = 1.5  # gradient threshold
    top_k = 10  # candidates per entity for cross-encoder reranking
    merge_map: dict[str, str] = {}  # alias -> canonical

    for i, key in enumerate(keys):
        if key in merge_map:
            continue
        bienc_scores = sim_matrix[i]
        # Stage 1: biencoder top-k candidates (fast)
        candidate_idxs = np.argsort(bienc_scores)[::-1][:top_k]
        candidates = [(int(idx), float(bienc_scores[idx])) for idx in candidate_idxs
                       if float(bienc_scores[idx]) >= 0.3]
        if not candidates:
            continue

        # Stage 2: cross-encoder reranking for sharper score separation
        query_text = texts[i]
        doc_texts = [texts[idx] for idx, _ in candidates]
        ce_scores = list(reranker.rerank(query_text, doc_texts))
        # ce_scores is a list of floats in same order as doc_texts
        reranked: list[tuple[int, float]] = [
            (candidates[j][0], float(ce_scores[j]))
            for j in range(len(candidates))
        ]
        reranked.sort(key=lambda x: -x[1])

        if not reranked:
            continue

        # Stage 3: gradient detection on cross-encoder scores
        selection: list[int] = []
        for j, (idx, s) in enumerate(reranked):
            if s < 0.5:  # minimum cross-encoder score threshold
                break
            if j == 0:
                selection.append(idx)
                continue
            prev_s = reranked[j - 1][1]
            if prev_s > 0 and s > prev_s / g:
                selection.append(idx)
            else:
                break  # sharp drop detected

        if not selection:
            continue  # Case A: new entity, no merge

        # Check if selection covers all above-threshold candidates (Case A)
        n_above_threshold = sum(1 for _, s in reranked if s >= 0.5)
        if len(selection) >= n_above_threshold:
            continue  # no sharp drop = all similar = too ambiguous to merge

        # Case B: merge with top candidate (keep longest name as canonical)
        top_idx = selection[0]
        target_key = keys[top_idx]
        if target_key in merge_map:
            target_key = merge_map[target_key]
        if target_key == key:
            continue

        # Keep longer name as canonical
        if len(key) >= len(target_key):
            canonical, alias = key, target_key
        else:
            canonical, alias = target_key, key

        # Remap any existing merges pointing to alias
        for k, v in merge_map.items():
            if v == alias:
                merge_map[k] = canonical
        merge_map[alias] = canonical

    # Apply merges
    for alias, canonical in merge_map.items():
        if alias in index.entities and canonical in index.entities:
            _merge_entity_into(index, canonical, alias)

    # Cache entity embeddings for fast query-time lookup
    final_keys = list(index.entities.keys())
    if final_keys:
        final_texts = []
        for k in final_keys:
            ent = index.entities[k]
            text = ent.name
            if ent.description:
                text += ": " + ent.description
            final_texts.append(text)
        index.entity_embeddings = embed_texts(final_texts, prefix="passage: ")


# ---------------------------------------------------------------------------
# CLI indexing pipeline (standalone, no MCP)
# ---------------------------------------------------------------------------


async def build_index(tree: dict[str, Any], name: str, *, use_llm: bool = False) -> Path:
    """Full indexing pipeline (CLI only, always local summaries)."""
    print("1. Parsing section tree...")
    index = parse_sections(tree)
    n_leaves = sum(1 for n in index.nodes.values() if n.kind != "section")
    n_sections = sum(1 for n in index.nodes.values() if n.kind == "section")
    print(f"   {n_sections} sections, {n_leaves} leaf nodes")

    print(f"2. Embedding {n_leaves} leaf nodes...")
    embed_leaves(index)

    print("3. Summarizing sections (local, no LLM)...")
    summarize_local(index)

    print(f"4. Embedding {n_sections} section summaries...")
    embed_summaries(index)

    print("5. Building entity index...")
    build_entity_index(index)
    print(f"   {len(index.entities)} unique entities")

    print("6. Saving index...")
    out = save_index(index, name)
    print(f"   Saved to {out}")
    return out


# ---------------------------------------------------------------------------
# MCP Tools: query helpers
# ---------------------------------------------------------------------------


def cosine_similarity(query_vec: NDArray[np.float32], matrix: NDArray[np.float32]) -> NDArray[np.float32]:
    """Cosine similarity between a query vector and a matrix of vectors."""
    norms = np.linalg.norm(matrix, axis=1, keepdims=True)
    norms = np.where(norms == 0, 1, norms)
    normed = matrix / norms
    q_norm = query_vec / max(np.linalg.norm(query_vec), 1e-10)
    return normed @ q_norm


def _parent_chain(index: BookIndex, node_id: str) -> list[IndexNode]:
    """Walk up from node to root, returning ancestors (root first)."""
    chain = []
    current = index.nodes.get(node_id)
    while current and current.parent_id:
        parent = index.nodes.get(current.parent_id)
        if parent:
            chain.append(parent)
        current = parent
    chain.reverse()
    return chain


def _format_node_content(index: BookIndex, node: IndexNode) -> str:
    """Format a node's full leaf content as markdown."""
    if node.kind != "section":
        return node.text
    parts = []
    for cid in node.children:
        child = index.nodes[cid]
        if child.kind == "section":
            parts.append(f"### {child.title}\n\n{_format_node_content(index, child)}")
        else:
            parts.append(child.text)
    return "\n\n".join(parts)


def _descendant_count(index: BookIndex, node_id: str) -> int:
    """Count total descendants (recursive)."""
    node = index.nodes.get(node_id)
    if not node:
        return 0
    count = len(node.children)
    for cid in node.children:
        count += _descendant_count(index, cid)
    return count


def _text_scores(
    index: BookIndex, query: str, *, node_ids: set[str] | None = None,
) -> dict[str, float]:
    """Compute text retrieval scores for nodes (S_T).

    When *node_ids* is provided, only those nodes are considered and scored
    (no global top-k truncation).  Otherwise the global top-30 are returned.
    """
    if index.embeddings is None or len(index.embeddings) == 0:
        return {}

    q_vec = embed_texts([query], prefix="query: ")[0]
    scores = cosine_similarity(q_vec, index.embeddings)

    idx_to_nid: dict[int, str] = {}
    for nid, node in index.nodes.items():
        if node.embedding_idx is not None:
            idx_to_nid[node.embedding_idx] = nid

    if node_ids is not None:
        # Scoped mode: score only the requested nodes, no global truncation
        candidates = [
            (emb_idx, nid)
            for emb_idx, nid in idx_to_nid.items()
            if nid in node_ids
        ]
    else:
        # Global mode: top-30
        top_indices = np.argsort(scores)[::-1][:30]
        candidates = [
            (int(i), idx_to_nid[int(i)])
            for i in top_indices
            if int(i) in idx_to_nid
        ]

    result: dict[str, float] = {}
    for emb_idx, nid in candidates:
        node = index.nodes[nid]
        raw_score = float(scores[emb_idx])
        if node.kind == "section":
            n_desc = _descendant_count(index, nid)
            penalty = 1.0 / (1.0 + math.log1p(n_desc) * 0.15)
            adjusted = raw_score * penalty
        else:
            adjusted = raw_score
        result[nid] = adjusted
    return result


def _graph_scores(index: BookIndex, start_entities: list[str], hops: int = 2) -> dict[str, float]:
    """Compute graph reasoning scores for all nodes (S_G) via personalized PageRank."""
    if not index.relations or not index.entities:
        return {}

    entity_keys = list(index.entities.keys())
    key_to_idx: dict[str, int] = {k: i for i, k in enumerate(entity_keys)}
    n = len(entity_keys)

    adj = np.zeros((n, n), dtype=np.float32)
    for rel in index.relations:
        si = key_to_idx.get(rel.source)
        ti = key_to_idx.get(rel.target)
        if si is not None and ti is not None and si != ti:
            adj[si, ti] += 1.0
            adj[ti, si] += 1.0

    row_sums = adj.sum(axis=1, keepdims=True)
    row_sums = np.where(row_sums == 0, 1, row_sums)
    transition = adj / row_sums

    alpha = 0.15
    personalization = np.zeros(n, dtype=np.float32)
    for ename in start_entities:
        key = ename.lower().strip()
        if key in key_to_idx:
            personalization[key_to_idx[key]] = 1.0
        else:
            for k in entity_keys:
                if key in k or k in key:
                    personalization[key_to_idx[k]] = 1.0
                    break

    if personalization.sum() == 0:
        return {}

    personalization /= personalization.sum()

    rank = personalization.copy()
    for _ in range(20 + hops * 10):
        rank = (1 - alpha) * (transition.T @ rank) + alpha * personalization
        rank /= max(rank.sum(), 1e-10)

    node_scores: dict[str, float] = {}
    for i, key in enumerate(entity_keys):
        entity_importance = float(rank[i])
        if entity_importance < 1e-6:
            continue
        ent = index.entities[key]
        for nid in ent.node_ids:
            if nid in index.nodes:
                node_scores[nid] = node_scores.get(nid, 0.0) + entity_importance

    return node_scores


def tool_search(index: BookIndex, query: str) -> str:
    """Collapsed tree retrieval with automatic skyline ranking when KG exists."""
    has_kg = bool(index.relations and index.entities)

    st = _text_scores(index, query)
    if not st:
        return "No embeddings in index."

    # When KG is available, auto-extract entities and merge via skyline
    sg: dict[str, float] = {}
    entity_names: list[str] = []
    if has_kg:
        entity_names = _extract_query_entity_names(index, query, top_k=5)
        if entity_names:
            sg = _graph_scores(index, entity_names)

    if sg:
        # Skyline merge: normalize, Pareto frontier, sort by sum
        all_nids = set(st) | set(sg)
        raw: list[tuple[str, float, float]] = [
            (nid, st.get(nid, 0.0), sg.get(nid, 0.0)) for nid in all_nids
        ]
        max_t = max(s for _, s, _ in raw) or 1.0
        max_g = max(s for _, _, s in raw) or 1.0
        scored: list[tuple[str, float, float]] = [
            (nid, t / max_t, g / max_g) for nid, t, g in raw
        ]
        # Pareto frontier
        non_dominated: list[tuple[str, float, float]] = []
        for nid_a, t_a, g_a in scored:
            dominated = False
            for _, t_b, g_b in scored:
                if (t_b >= t_a and g_b >= g_a) and (t_b > t_a or g_b > g_a):
                    dominated = True
                    break
            if not dominated:
                non_dominated.append((nid_a, t_a, g_a))
        non_dominated.sort(key=lambda x: -(x[1] + x[2]))
        hits: list[tuple[str, float, float | None]] = [
            (nid, t, g) for nid, t, g in non_dominated[:10]
        ]
    else:
        # Text-only fallback
        sorted_st = sorted(st.items(), key=lambda x: -x[1])[:10]
        hits = [(nid, score, None) for nid, score in sorted_st]

    if not hits:
        return "No results found."

    # Collapsed tree: group by shared parents, deduplicate
    seen_ids: set[str] = set()
    groups: dict[str, list[tuple[IndexNode, float, float | None, list[IndexNode]]]] = {}

    for nid, t_score, g_score in hits:
        if nid in seen_ids:
            continue
        seen_ids.add(nid)
        node = index.nodes[nid]
        chain = _parent_chain(index, nid)
        group_key = chain[0].id if chain else nid
        groups.setdefault(group_key, []).append((node, t_score, g_score, chain))

    # Format output
    parts = [f"# Search results for: {query}\n"]
    if entity_names and sg:
        parts.append(f"*Skyline ranking with entities: {', '.join(entity_names)}*\n")
    for group_key, group_hits in groups.items():
        group_node = index.nodes[group_key]
        parts.append(f"## {group_node.title or 'Untitled'}")
        if group_node.text:
            parts.append(f"*{group_node.text[:200]}*\n")

        for node, t_score, g_score, chain in group_hits:
            breadcrumb = " > ".join(a.title for a in chain if a.title)
            label = node.title or node.kind
            if g_score is not None:
                score_str = f"S_T: {t_score:.3f}, S_G: {g_score:.3f}"
            else:
                score_str = f"score: {t_score:.3f}"
            if breadcrumb:
                parts.append(f"**{breadcrumb} > {label}** [{node.id}] ({score_str})")
            else:
                parts.append(f"**{label}** [{node.id}] ({score_str})")

            preview = node.text[:300] if node.text else "(no content)"
            parts.append(f"{preview}\n")

    return "\n".join(parts)


def tool_browse(index: BookIndex) -> str:
    """Table of contents with summaries."""
    parts = [f"# {index.title}"]
    if index.authors:
        parts.append(f"*By {', '.join(index.authors)}*\n")

    def _walk(nid: str, depth: int) -> None:
        node = index.nodes[nid]
        indent = "  " * depth
        title = node.title or node.kind
        parts.append(f"{indent}- **{title}**")
        if node.text and node.kind == "section":
            summary = node.text[:150]
            parts.append(f"{indent}  {summary}")
        for cid in node.children:
            child = index.nodes[cid]
            if child.kind == "section":
                _walk(cid, depth + 1)

    for rid in index.root_ids:
        _walk(rid, 0)

    return "\n".join(parts)


def tool_get_section(index: BookIndex, title: str) -> str:
    """Full content of a section by fuzzy title match."""
    query_lower = title.lower()
    query_words = set(query_lower.split())
    best_score = 0.0
    best_node: IndexNode | None = None

    for node in index.nodes.values():
        if node.kind != "section" or not node.title:
            continue
        node_lower = node.title.lower()
        node_words = set(node_lower.split())
        if not query_words or not node_words:
            continue
        # Word overlap (Jaccard)
        overlap = len(query_words & node_words)
        score = overlap / max(len(query_words | node_words), 1)
        # Boost for substring containment
        if query_lower in node_lower:
            score = max(score, 0.5 + 0.5 * len(query_lower) / max(len(node_lower), 1))
        elif node_lower in query_lower:
            score = max(score, 0.5 + 0.5 * len(node_lower) / max(len(query_lower), 1))
        if score > best_score:
            best_score = score
            best_node = node

    if not best_node or best_score < 0.1:
        return f"No section matching '{title}' found."

    content = _format_node_content(index, best_node)
    chain = _parent_chain(index, best_node.id)
    breadcrumb = " > ".join(a.title for a in chain if a.title)
    header = f"{breadcrumb} > {best_node.title}" if breadcrumb else best_node.title

    parts = [f"# {header}"]
    if best_node.text:
        parts.append(f"*Summary: {best_node.text}*\n")
    parts.append(content)
    return "\n".join(parts)


def tool_find_related(index: BookIndex, topic: str) -> str:
    """Entity-based cross-chapter discovery via GT-Links."""
    topic_lower = topic.lower().strip()

    # Match entities: exact -> substring -> embedding similarity
    matched_entities: list[Entity] = []

    # Exact match
    if topic_lower in index.entities:
        matched_entities.append(index.entities[topic_lower])

    # Substring match
    if not matched_entities:
        for key, ent in index.entities.items():
            if topic_lower in key or key in topic_lower:
                matched_entities.append(ent)

    # Embedding similarity fallback
    if not matched_entities and index.entities:
        q_vec = embed_texts([topic], prefix="query: ")[0]
        keys = list(index.entities.keys())
        if index.entity_embeddings is not None and len(index.entity_embeddings) == len(keys):
            ent_vecs = index.entity_embeddings
        elif index.embeddings is not None:
            # Legacy fallback: use node embeddings
            entity_nodes: list[tuple[Entity, str]] = []
            for ent in index.entities.values():
                for nid in ent.node_ids:
                    node = index.nodes.get(nid)
                    if node and node.embedding_idx is not None:
                        entity_nodes.append((ent, nid))
                        break
            if entity_nodes:
                idxs = [index.nodes[nid].embedding_idx for _, nid in entity_nodes]
                sub_matrix = index.embeddings[idxs]
                scores = cosine_similarity(q_vec, sub_matrix)
                top = np.argsort(scores)[::-1][:5]
                for i in top:
                    if scores[int(i)] > 0.3:
                        matched_entities.append(entity_nodes[int(i)][0])
            ent_vecs = None
        else:
            ent_vecs = None
        if ent_vecs is not None:
            scores = cosine_similarity(q_vec, ent_vecs)
            top = np.argsort(scores)[::-1][:5]
            for i in top:
                if scores[int(i)] > 0.3:
                    matched_entities.append(index.entities[keys[int(i)]])

    if not matched_entities:
        return f"No entities matching '{topic}' found."

    # Collect sections via GT-Links, group by chapter
    chapters: dict[str, list[tuple[IndexNode, int]]] = {}
    for ent in matched_entities:
        for nid in ent.node_ids:
            node = index.nodes.get(nid)
            if not node or node.kind != "section":
                continue
            chain = _parent_chain(index, nid)
            chapter = chain[0].title if chain else node.title
            if chapter not in chapters:
                chapters[chapter] = []
            # Count entity mentions for ranking
            mention_count = sum(1 for e in matched_entities if nid in e.node_ids)
            chapters[chapter].append((node, mention_count))

    if not chapters:
        return f"Entities found but no linked sections for '{topic}'."

    parts = [f"# Sections related to: {topic}\n"]
    parts.append(f"Matched entities: {', '.join(e.name for e in matched_entities)}\n")

    # Sort chapters by total mentions
    sorted_chapters = sorted(chapters.items(), key=lambda x: sum(m for _, m in x[1]), reverse=True)
    for chapter, sections in sorted_chapters:
        parts.append(f"## {chapter}")
        # Deduplicate and sort by mentions
        seen: set[str] = set()
        unique = []
        for node, count in sections:
            if node.id not in seen:
                seen.add(node.id)
                unique.append((node, count))
        unique.sort(key=lambda x: x[1], reverse=True)
        for node, count in unique:
            summary = node.text[:200] if node.text else ""
            parts.append(f"- **{node.title}** ({count} entity mentions)")
            if summary:
                parts.append(f"  {summary}")

    return "\n".join(parts)


# ---------------------------------------------------------------------------
# KG operator helpers
# ---------------------------------------------------------------------------


def _extract_query_entity_names(index: BookIndex, query: str, top_k: int = 5) -> list[str]:
    """Extract top-k entity names matching a query via embedding similarity.

    Returns entity keys (lowercase) with cosine similarity >= 0.3.
    """
    if not index.entities:
        return []

    keys = list(index.entities.keys())
    q_vec = embed_texts([query], prefix="query: ")[0]
    if index.entity_embeddings is not None and len(index.entity_embeddings) == len(keys):
        ent_vecs = index.entity_embeddings
    else:
        texts = []
        for k in keys:
            ent = index.entities[k]
            text = ent.name
            if ent.description:
                text += ": " + ent.description
            texts.append(text)
        ent_vecs = embed_texts(texts, prefix="passage: ")
    scores = cosine_similarity(q_vec, ent_vecs)

    top_idxs = np.argsort(scores)[::-1][:top_k]
    result: list[str] = []
    for idx in top_idxs:
        idx_int = int(idx)
        if float(scores[idx_int]) >= 0.3:
            result.append(keys[idx_int])
    return result


def tool_extract_query_entities(index: BookIndex, query: str) -> str:
    """Formulator: Extract — find entities relevant to a query via embedding similarity."""
    if not index.entities:
        return "No entities in index."

    keys = list(index.entities.keys())
    q_vec = embed_texts([query], prefix="query: ")[0]
    if index.entity_embeddings is not None and len(index.entity_embeddings) == len(keys):
        ent_vecs = index.entity_embeddings
    else:
        texts = []
        for k in keys:
            ent = index.entities[k]
            text = ent.name
            if ent.description:
                text += ": " + ent.description
            texts.append(text)
        ent_vecs = embed_texts(texts, prefix="passage: ")
    scores = cosine_similarity(q_vec, ent_vecs)

    top_idxs = np.argsort(scores)[::-1][:10]

    parts = [f"# Query entities for: {query}\n"]
    for idx in top_idxs:
        idx_int = int(idx)
        score = float(scores[idx_int])
        if score < 0.3:
            break
        key = keys[idx_int]
        ent = index.entities[key]

        # Find relations involving this entity
        related_rels = [r for r in index.relations if r.source == key or r.target == key]

        parts.append(f"**{ent.name}** (score: {score:.3f})")
        if ent.description:
            parts.append(f"  {ent.description}")
        parts.append(f"  Linked to {len(ent.node_ids)} nodes")

        if related_rels:
            rel_strs = []
            for r in related_rels[:5]:
                if r.source == key:
                    rel_strs.append(f"{r.source} —[{r.rel_type}]→ {r.target}")
                else:
                    rel_strs.append(f"{r.source} —[{r.rel_type}]→ {r.target}")
            parts.append(f"  Relations: {'; '.join(rel_strs)}")
        parts.append("")

    if len(parts) == 1:
        return f"No entities matching query '{query}' (threshold 0.3)."

    return "\n".join(parts)


def tool_select_by_entity(index: BookIndex, entity_name: str) -> str:
    """Selector: Select_by_Entity — find sections linked to an entity via GT-Links."""
    key = entity_name.lower().strip()

    # Find entity: exact -> substring
    ent = index.entities.get(key)
    if not ent:
        for k, e in index.entities.items():
            if key in k or k in key:
                ent = e
                break

    if not ent:
        return f"No entity matching '{entity_name}' found."

    # Get all section nodes via GT-Links
    sections: list[tuple[IndexNode, list[IndexNode]]] = []
    for nid in ent.node_ids:
        node = index.nodes.get(nid)
        if not node or node.kind != "section":
            continue
        chain = _parent_chain(index, nid)
        sections.append((node, chain))

    if not sections:
        return f"Entity '{ent.name}' found but no linked sections."

    parts = [f"# Sections for entity: {ent.name}\n"]
    if ent.description:
        parts.append(f"*{ent.description}*\n")

    # Show relations
    rels = [r for r in index.relations if r.source == key or r.target == key]
    if rels:
        parts.append("**Relations:**")
        seen_rels: set[str] = set()
        for r in rels:
            rel_str = f"  {r.source} —[{r.rel_type}]→ {r.target}"
            if rel_str not in seen_rels:
                seen_rels.add(rel_str)
                parts.append(rel_str)
                if len(seen_rels) >= 15:
                    parts.append(f"  ...and {len(rels) - 15} more")
                    break
        parts.append("")

    # Group by top-level ancestor
    groups: dict[str, list[tuple[IndexNode, list[IndexNode]]]] = {}
    for node, chain in sections:
        group_key = chain[0].title if chain else node.title
        groups.setdefault(group_key, []).append((node, chain))

    for group_title, group_sections in groups.items():
        parts.append(f"## {group_title}")
        for node, chain in group_sections:
            breadcrumb = " > ".join(a.title for a in chain if a.title)
            label = node.title or node.kind
            loc = f"{breadcrumb} > {label}" if breadcrumb else label
            summary = node.text[:200] if node.text else ""
            parts.append(f"- **{loc}** [{node.id}]")
            if summary:
                parts.append(f"  {summary}")

    return "\n".join(parts)


def tool_graph_reason(index: BookIndex, start_entities: list[str], hops: int = 2) -> str:
    """Reasoner: Graph_Reasoning — personalized PageRank over the KG."""
    if not index.relations:
        return "No relations in knowledge graph. Run relation extraction first."

    node_scores = _graph_scores(index, start_entities, hops)
    if not node_scores:
        return f"None of the start entities found or no nodes scored: {start_entities}"

    sorted_nodes = sorted(node_scores.items(), key=lambda x: -x[1])[:15]

    # Resolve which start entities were found
    found_starts = []
    for ename in start_entities:
        key = ename.lower().strip()
        if key in index.entities:
            found_starts.append(key)
        else:
            for k in index.entities:
                if key in k or k in key:
                    found_starts.append(k)
                    break

    parts = [f"# Graph reasoning from: {', '.join(found_starts)}\n"]
    parts.append(f"PageRank with {hops} hops, {len(index.relations)} relations\n")

    parts.append("**Top nodes by graph score:**")
    for nid, score in sorted_nodes:
        node = index.nodes[nid]
        chain = _parent_chain(index, nid)
        breadcrumb = " > ".join(a.title for a in chain if a.title)
        label = node.title or node.kind
        loc = f"{breadcrumb} > {label}" if breadcrumb else label
        parts.append(f"- **{loc}** [{nid}] (graph_score: {score:.4f})")
        if node.text:
            parts.append(f"  {node.text[:200]}")

    return "\n".join(parts)


def tool_skyline_rank(
    index: BookIndex, query: str, start_entities: list[str], hops: int = 2,
) -> str:
    """Skyline_Ranker: Pareto frontier over text (S_T) and graph (S_G) scores."""
    st = _text_scores(index, query)
    sg = _graph_scores(index, start_entities, hops)

    # Union of all scored nodes (use 0 for missing)
    all_nids = set(st) | set(sg)
    if not all_nids:
        return "No scored nodes from text or graph signals."

    raw: list[tuple[str, float, float]] = []
    for nid in all_nids:
        raw.append((nid, st.get(nid, 0.0), sg.get(nid, 0.0)))

    # Normalize each dimension to [0, 1]
    max_t = max(s for _, s, _ in raw) or 1.0
    max_g = max(s for _, _, s in raw) or 1.0
    scored: list[tuple[str, float, float]] = [
        (nid, t / max_t, g / max_g) for nid, t, g in raw
    ]

    # Pareto frontier: keep non-dominated nodes
    # A dominates B iff A >= B on all dims and A > B on at least one
    non_dominated: list[tuple[str, float, float]] = []
    for nid_a, t_a, g_a in scored:
        dominated = False
        for _, t_b, g_b in scored:
            if (t_b >= t_a and g_b >= g_a) and (t_b > t_a or g_b > g_a):
                dominated = True
                break
        if not dominated:
            non_dominated.append((nid_a, t_a, g_a))

    non_dominated.sort(key=lambda x: -(x[1] + x[2]))

    parts = [f"# Skyline ranking for: {query}\n"]
    parts.append(f"Text candidates: {len(st)}, Graph candidates: {len(sg)}, "
                 f"Pareto frontier: {len(non_dominated)}\n")

    for nid, t_score, g_score in non_dominated:
        node = index.nodes[nid]
        chain = _parent_chain(index, nid)
        breadcrumb = " > ".join(a.title for a in chain if a.title)
        label = node.title or node.kind
        loc = f"{breadcrumb} > {label}" if breadcrumb else label
        parts.append(
            f"- **{loc}** [{nid}] (S_T: {t_score:.3f}, S_G: {g_score:.3f}, "
            f"sum: {t_score + g_score:.3f})"
        )
        if node.text:
            parts.append(f"  {node.text[:200]}")

    return "\n".join(parts)


def _collect_descendants(index: BookIndex, node_id: str) -> set[str]:
    """Recursively collect all child IDs under a node."""
    result: set[str] = set()
    node = index.nodes.get(node_id)
    if not node:
        return result
    for cid in node.children:
        result.add(cid)
        result.update(_collect_descendants(index, cid))
    return result


def tool_select_by_section(index: BookIndex, section_title: str, query: str) -> str:
    """Select_by_Section: scope retrieval to a section subtree."""
    # Fuzzy-match section_title (reuse logic from tool_get_section)
    query_lower = section_title.lower()
    query_words = set(query_lower.split())
    best_score = 0.0
    best_node: IndexNode | None = None

    for node in index.nodes.values():
        if node.kind != "section" or not node.title:
            continue
        node_lower = node.title.lower()
        node_words = set(node_lower.split())
        if not query_words or not node_words:
            continue
        overlap = len(query_words & node_words)
        score = overlap / max(len(query_words | node_words), 1)
        if query_lower in node_lower:
            score = max(score, 0.5 + 0.5 * len(query_lower) / max(len(node_lower), 1))
        elif node_lower in query_lower:
            score = max(score, 0.5 + 0.5 * len(node_lower) / max(len(query_lower), 1))
        if score > best_score:
            best_score = score
            best_node = node

    if not best_node or best_score < 0.1:
        return f"No section matching '{section_title}' found."

    # Collect all descendant node IDs
    descendants = _collect_descendants(index, best_node.id)
    descendants.add(best_node.id)

    # Run text scores scoped to descendants only (no global top-k truncation)
    scoped_scores = _text_scores(index, query, node_ids=descendants)

    if not scoped_scores:
        return f"No matching content within section '{best_node.title}' for query '{query}'."

    hits = sorted(scoped_scores.items(), key=lambda x: -x[1])[:10]

    # Format results with breadcrumbs relative to the scoping section
    scope_chain = _parent_chain(index, best_node.id)
    scope_prefix_ids = {a.id for a in scope_chain}
    scope_prefix_ids.add(best_node.id)

    parts = [f"# Results within: {best_node.title}\n"]
    parts.append(f"*Scoped search for: {query}*\n")

    for nid, score in hits:
        node = index.nodes[nid]
        chain = _parent_chain(index, nid)
        # Show breadcrumb relative to scoping section
        relative_chain = [a for a in chain if a.id not in scope_prefix_ids]
        breadcrumb = " > ".join(a.title for a in relative_chain if a.title)
        label = node.title or node.kind
        if breadcrumb:
            parts.append(f"**{breadcrumb} > {label}** [{nid}] (score: {score:.3f})")
        else:
            parts.append(f"**{label}** [{nid}] (score: {score:.3f})")
        preview = node.text[:300] if node.text else "(no content)"
        parts.append(f"{preview}\n")

    return "\n".join(parts)


# ---------------------------------------------------------------------------
# Multi-book resolution
# ---------------------------------------------------------------------------


def _resolve_book(books: dict[str, BookIndex], book: str) -> tuple[str, BookIndex] | str:
    """Resolve a book name: exact match -> substring on name/title. Returns error string on failure."""
    if book in books:
        return book, books[book]
    # Substring match on name or title
    query = book.lower()
    matches = []
    for name, idx in books.items():
        if query in name.lower() or query in idx.title.lower():
            matches.append((name, idx))
    if len(matches) == 1:
        return matches[0]
    available = ", ".join(f"{n} ({idx.title})" for n, idx in books.items())
    if not available:
        available = "(none)"
    if len(matches) > 1:
        matched_names = ", ".join(f"{n} ({idx.title})" for n, idx in matches)
        return f"Ambiguous book name '{book}', matches: {matched_names}. Available books: {available}"
    return f"No book matching '{book}'. Available books: {available}"


# ---------------------------------------------------------------------------
# MCP Server (multi-book)
# ---------------------------------------------------------------------------


def run_server() -> None:
    """Start multi-book MCP server with indexing + query tools."""
    from mcp.server.fastmcp import FastMCP

    books = load_all_indexes()
    print(f"Loaded {len(books)} book(s): {', '.join(books.keys()) or '(none)'}", file=sys.stderr)

    mcp = FastMCP("BookRAG")

    # -- Indexing tools --

    @mcp.tool()
    def index_book(file_path: str) -> str:
        """Index an epub/azw3/mobi file for semantic search.

        Runs boko to extract sections, embeds leaf nodes, and saves a partial
        index. After this completes, call get_sections_to_summarize() to begin
        generating summaries (you are the LLM — summarize each batch and submit
        via submit_summaries).
        """
        path = Path(file_path).expanduser().resolve()
        if not path.exists():
            return f"File not found: {path}"

        # Derive index name from filename
        name = path.stem.lower().replace(" ", "-")
        if name in books:
            return f"Book '{name}' is already indexed. Use list_books() to see all books."

        # Run boko sections
        boko = shutil.which("boko")
        if not boko:
            return "Error: 'boko' not found on PATH. Install boko first."
        try:
            result = subprocess.run(
                [boko, "sections", str(path)],
                capture_output=True, text=True, timeout=120,
            )
        except subprocess.TimeoutExpired:
            return "Error: boko sections timed out after 120s."
        if result.returncode != 0:
            return f"Error running boko sections: {result.stderr[:500]}"

        tree = json.loads(result.stdout)
        index = parse_sections(tree)
        n_leaves = sum(1 for n in index.nodes.values() if n.kind != "section")
        n_sections = sum(1 for n in index.nodes.values() if n.kind == "section")

        # Embed leaves
        embed_leaves(index)

        # Save partial index (no summaries yet)
        save_index(index, name)
        books[name] = index

        return (
            f"Indexed '{index.title}' by {', '.join(index.authors) or 'unknown'} as '{name}'.\n"
            f"{n_sections} sections, {n_leaves} leaf nodes, embeddings computed.\n\n"
            f"Now generate summaries:\n"
            f"1. Call get_sections_to_summarize(book='{name}') to get batches of section IDs\n"
            f"2. For one batch: get_section_content → summarize → submit_summaries\n"
            f"3. Repeat step 2 for each batch. Process one batch at a time to save context.\n"
            f"4. Call get_sections_to_summarize again when all batches are done (higher\n"
            f"   levels become ready as children are summarized). Repeat until complete."
        )

    @mcp.tool()
    def get_sections_to_summarize(book: str) -> str:
        """Get a manifest of unsummarized sections ready for summarization.

        Returns a lightweight list of section IDs, titles, levels, and content
        sizes — no full content. Use this to plan work distribution across
        parallel subagents, then have each subagent call get_section_content()
        to retrieve the actual text for its assigned sections.

        Sections are ordered deepest-level-first for bottom-up summarization.
        A section is "ready" when all its child sections are already summarized.

        Returns empty message when all sections are summarized.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        name, index = resolved

        # Find unsummarized sections
        unsummarized: list[IndexNode] = []
        for n in index.nodes.values():
            if n.kind == "section" and not n.text:
                unsummarized.append(n)

        if not unsummarized:
            return f"All sections in '{name}' are summarized! The index is complete."

        total = sum(1 for n in index.nodes.values() if n.kind == "section")
        done = total - len(unsummarized)

        # Only include sections whose children are all leaves or already-summarized
        ready: list[IndexNode] = []
        for node in unsummarized:
            children_ready = True
            for cid in node.children:
                child = index.nodes[cid]
                if child.kind == "section" and not child.text:
                    children_ready = False
                    break
            if children_ready:
                ready.append(node)

        # Sort by level descending (deepest first), stable within level
        ready.sort(key=lambda n: -n.level)

        if not ready:
            return f"No ready sections yet ({done}/{total} done). Child sections must be summarized first."

        # Compute content sizes and partition into balanced batches
        section_sizes: list[tuple[str, int]] = []  # (id, content_chars)
        for node in ready:
            content = _gather_leaf_content(index, node)
            section_sizes.append((node.id, len(content)))

        total_chars = sum(size for _, size in section_sizes)
        n_batches = max(1, round(total_chars / 10_000))  # ~10K chars per batch

        # Greedy bin-packing: largest-first into smallest batch
        batches: list[list[str]] = [[] for _ in range(n_batches)]
        batch_sizes = [0] * n_batches
        for sid, size in sorted(section_sizes, key=lambda x: -x[1]):
            smallest = min(range(n_batches), key=lambda i: batch_sizes[i])
            batches[smallest].append(sid)
            batch_sizes[smallest] += size

        return json.dumps({
            "book": name,
            "done": done,
            "total": total,
            "remaining": len(unsummarized),
            "batches": [
                {"section_ids": ids, "total_chars": size}
                for ids, size in zip(batches, batch_sizes)
                if ids
            ],
        })

    @mcp.tool()
    def get_section_content(book: str, section_ids: str) -> str:
        """Get full content for specific sections to summarize.

        Called by subagents after the orchestrator assigns them section IDs
        from the get_sections_to_summarize() manifest.

        Args:
            book: Book name
            section_ids: JSON array of section ID strings

        For each section, generate and submit via submit_summaries():
        - A 2-4 sentence summary optimized for retrieval (include distinctive terms)
        - A list of named entities (people, places, concepts) with 1-sentence descriptions
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved

        try:
            ids = json.loads(section_ids)
        except json.JSONDecodeError as e:
            return f"Invalid JSON: {e}"

        if not isinstance(ids, list):
            return "Expected a JSON array of node ID strings."

        parts = []
        for sid in ids:
            node = index.nodes.get(sid)
            if not node:
                parts.append(f"## Unknown ID: {sid}\n")
                continue
            if node.kind == "section":
                content = _gather_leaf_content(index, node)
                if len(content) > 8000:
                    content = content[:8000] + "\n[...truncated]"
                parts.append(f"## Section: {node.title or '(untitled)'}")
                parts.append(f"ID: {node.id}")
                parts.append(f"Level: {node.level}")
                parts.append(f"Content:\n{content}\n")
            else:
                # Leaf node — return text directly with parent context
                chain = _parent_chain(index, sid)
                context = " > ".join(a.title for a in chain if a.title)
                text = node.text[:3000]
                parts.append(f"## {context or node.kind}")
                parts.append(f"ID: {node.id}")
                parts.append(f"Content:\n{text}\n")

        return "\n".join(parts)

    @mcp.tool()
    def submit_summaries(book: str, summaries: str) -> str:
        """Submit summaries for a batch of sections.

        Args:
            book: Book name
            summaries: JSON array of objects with:
                - section_id: ID of the section
                - summary: 2-4 sentence summary
                - entities: array of {name, description} objects
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        name, index = resolved

        try:
            data = json.loads(summaries)
        except json.JSONDecodeError as e:
            return f"Invalid JSON: {e}"

        if not isinstance(data, list):
            return "Expected a JSON array of summary objects."

        submitted = 0
        errors = []
        for entry in data:
            sid = entry.get("section_id", "")
            summary = entry.get("summary", "")
            entities = entry.get("entities", [])

            node = index.nodes.get(sid)
            if not node:
                errors.append(f"Unknown section_id: {sid}")
                continue
            if node.kind != "section":
                errors.append(f"{sid} is not a section")
                continue

            node.text = summary
            for ent in entities:
                ent_name = ent.get("name", "").strip()
                if ent_name:
                    node.entities.append(ent_name)
                    # Store entity description in index
                    key = ent_name.lower().strip()
                    if key not in index.entities:
                        index.entities[key] = Entity(
                            name=ent_name,
                            description=ent.get("description", ""),
                            node_ids=[node.id],
                        )
                    else:
                        ent_obj = index.entities[key]
                        if node.id not in ent_obj.node_ids:
                            ent_obj.node_ids.append(node.id)
                        if not ent_obj.description and ent.get("description"):
                            ent_obj.description = ent["description"]
            submitted += 1

        # Check if all sections are now summarized
        remaining = sum(1 for n in index.nodes.values() if n.kind == "section" and not n.text)
        total = sum(1 for n in index.nodes.values() if n.kind == "section")
        done = total - remaining

        if remaining == 0:
            # All done — finalize: embed summaries, build entity index, save
            embed_summaries(index)
            build_entity_index(index)
            save_index(index, name)
            msg = (
                f"Submitted {submitted} summaries. All {total} sections summarized!\n"
                f"Embedded summaries, built entity index ({len(index.entities)} entities), saved.\n"
                f"'{name}' is ready for queries."
            )
        else:
            # Save progress
            save_index(index, name)
            msg = (
                f"Submitted {submitted} summaries. Progress: {done}/{total} sections done, "
                f"{remaining} remaining.\n"
                f"Call get_sections_to_summarize(book='{name}') for the next batch."
            )

        if errors:
            msg += "\nErrors: " + "; ".join(errors)

        return msg

    # -- Relation extraction tools --

    @mcp.tool()
    def get_nodes_for_relation_extraction(book: str) -> str:
        """Get batches of leaf nodes for entity-relation extraction.

        Returns nodes with text content that haven't had relations extracted yet.
        For each node, extract:
        1. (source_entity, relation_type, target_entity) triples describing
           relationships between entities mentioned in the text.
        2. Any named entities (people, places, concepts) you encounter — include
           these as an "entities" field in submit_relations() with [{name, description}].

        Submit results via submit_relations(). When all nodes are processed,
        the knowledge graph is complete.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        name, index = resolved

        # Find leaf nodes not yet processed
        pending: list[IndexNode] = []
        for n in index.nodes.values():
            if n.kind != "section" and n.text and n.id not in index.relations_extracted_node_ids:
                pending.append(n)

        if not pending:
            n_rels = len(index.relations)
            return (
                f"All nodes in '{name}' have been processed for relation extraction. "
                f"{n_rels} relations in knowledge graph."
            )

        total_nodes = sum(1 for n in index.nodes.values() if n.kind != "section" and n.text)
        done = total_nodes - len(pending)

        # Partition into ~10K char batches
        node_sizes: list[tuple[str, int]] = [(n.id, len(n.text)) for n in pending]
        total_chars = sum(s for _, s in node_sizes)
        n_batches = max(1, round(total_chars / 10_000))

        batches: list[list[str]] = [[] for _ in range(n_batches)]
        batch_sizes = [0] * n_batches
        for nid, size in sorted(node_sizes, key=lambda x: -x[1]):
            smallest = min(range(n_batches), key=lambda i: batch_sizes[i])
            batches[smallest].append(nid)
            batch_sizes[smallest] += size

        return json.dumps({
            "book": name,
            "done": done,
            "total": total_nodes,
            "remaining": len(pending),
            "batches": [
                {"node_ids": ids, "total_chars": size}
                for ids, size in zip(batches, batch_sizes)
                if ids
            ],
        })

    @mcp.tool()
    def submit_relations(book: str, relations: str) -> str:
        """Submit extracted relations for a batch of nodes.

        Args:
            book: Book name
            relations: JSON object with:
                - node_ids: array of node ID strings that were processed
                - relations: array of {source, target, rel_type, node_id} objects
                - entities (optional): array of {name, description} objects for
                  entities discovered in these nodes
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        name, index = resolved

        try:
            data = json.loads(relations)
        except json.JSONDecodeError as e:
            return f"Invalid JSON: {e}"

        if not isinstance(data, dict):
            return "Expected a JSON object with 'node_ids' and 'relations' fields."

        processed_ids = data.get("node_ids", [])
        rel_list = data.get("relations", [])
        entity_list = data.get("entities", [])

        # Mark nodes as processed
        for nid in processed_ids:
            index.relations_extracted_node_ids.add(nid)

        # Register any new entities from leaf nodes
        for ent in entity_list:
            ent_name = ent.get("name", "").strip()
            if not ent_name:
                continue
            key = ent_name.lower().strip()
            if key not in index.entities:
                index.entities[key] = Entity(
                    name=ent_name,
                    description=ent.get("description", ""),
                    node_ids=list(processed_ids),
                )
            else:
                ent_obj = index.entities[key]
                for nid in processed_ids:
                    if nid not in ent_obj.node_ids:
                        ent_obj.node_ids.append(nid)
                if not ent_obj.description and ent.get("description"):
                    ent_obj.description = ent["description"]

        # Add relations
        added = 0
        errors = []
        for r in rel_list:
            source = r.get("source", "").lower().strip()
            target = r.get("target", "").lower().strip()
            rel_type = r.get("rel_type", "").strip()
            node_id = r.get("node_id", "")

            if not source or not target or not rel_type:
                errors.append(f"Skipped incomplete relation: {r}")
                continue

            # Ensure entities exist
            for key in (source, target):
                if key not in index.entities:
                    index.entities[key] = Entity(name=key, description="", node_ids=[])
                if node_id and node_id not in index.entities[key].node_ids:
                    index.entities[key].node_ids.append(node_id)

            index.relations.append(Relation(
                source=source, target=target, rel_type=rel_type, node_id=node_id,
            ))
            added += 1

        # Check progress
        total_nodes = sum(1 for n in index.nodes.values() if n.kind != "section" and n.text)
        done = len(index.relations_extracted_node_ids)
        remaining = total_nodes - done

        # Save progress
        save_index(index, name)

        msg = (
            f"Added {added} relations ({len(index.relations)} total). "
            f"Processed {done}/{total_nodes} nodes, {remaining} remaining."
        )

        if remaining == 0:
            # All done — rebuild entity index
            build_entity_index(index)
            save_index(index, name)
            msg += (
                f"\nAll nodes processed! Rebuilt entity index: "
                f"{len(index.entities)} entities, {len(index.relations)} relations."
            )
        else:
            msg += f"\nCall get_nodes_for_relation_extraction(book='{name}') for the next batch."

        if errors:
            msg += "\nWarnings: " + "; ".join(errors[:5])

        return msg

    # -- Query tools --

    @mcp.tool()
    def list_books() -> str:
        """List all indexed books with title, author, and stats."""
        if not books:
            return "No books indexed. Use index_book() to index an epub file."
        parts = ["# Indexed Books\n"]
        for name, idx in books.items():
            n_sections = sum(1 for n in idx.nodes.values() if n.kind == "section")
            n_leaves = sum(1 for n in idx.nodes.values() if n.kind != "section")
            n_summarized = sum(1 for n in idx.nodes.values() if n.kind == "section" and n.text)
            status = "ready" if n_summarized == n_sections else f"{n_summarized}/{n_sections} summarized"
            n_entities = len(idx.entities)
            n_relations = len(idx.relations)
            kg_status = f"{n_entities} entities, {n_relations} relations" if n_entities else "no KG"
            parts.append(
                f"- **{name}**: {idx.title} by {', '.join(idx.authors) or 'unknown'} "
                f"({n_sections} sections, {n_leaves} leaves, {status}, {kg_status})"
            )
        return "\n".join(parts)

    @mcp.tool()
    def search(query: str, book: str) -> str:
        """Semantic search across a book using collapsed tree retrieval.

        Embeds the query and finds the most relevant passages and sections,
        presenting results with hierarchical context (parent summaries).
        Use this to find specific information, arguments, or passages.

        This is the default starting point for most queries. When a knowledge
        graph is available, results automatically combine text similarity with
        graph importance (skyline ranking). For broad exploration, start with
        browse(). To narrow results to a specific chapter, use
        select_by_section().
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_search(index, query)

    @mcp.tool()
    def browse(book: str) -> str:
        """Browse a book's table of contents with section summaries.

        Returns a hierarchical outline of the book with summaries at each level.
        Use this first to orient yourself before searching for specific topics.
        Then use select_by_section() to scope searches to interesting chapters.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_browse(index)

    @mcp.tool()
    def get_section(title: str, book: str) -> str:
        """Get the full content of a section by title.

        Uses fuzzy matching to find the best-matching section and returns
        all of its content formatted as markdown. Use after search() or
        browse() to read a specific section in full. Prefer this over
        repeated searches when you know which section you need.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_get_section(index, title)

    @mcp.tool()
    def find_related(topic: str, book: str) -> str:
        """Find sections related to a topic via entity cross-references.

        Uses the entity knowledge graph to discover sections across different
        chapters that discuss the same people, places, concepts, or terms.
        Best for cross-chapter thematic exploration. For targeted retrieval
        within a chapter, use select_by_section() instead.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_find_related(index, topic)

    @mcp.tool()
    def select_by_section(query: str, book: str, section_title: str) -> str:
        """Search within a specific section of the book.

        Restricts semantic search to a section subtree. Use this after browse()
        to narrow results to a specific chapter, part, or topic area.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_select_by_section(index, section_title, query)

    # -- KG operator tools --

    @mcp.tool()
    def extract_query_entities(query: str, book: str) -> str:
        """Extract entities relevant to a query using embedding similarity.

        Embeds the query and finds top matching entities in the knowledge graph.
        Returns matched entities with descriptions, node links, and relations.
        Use this to understand what the KG knows about a topic before calling
        graph_reason() or skyline_rank(). Not needed before search() — search
        auto-extracts entities when a KG exists.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_extract_query_entities(index, query)

    @mcp.tool()
    def select_by_entity(book: str, entity_name: str) -> str:
        """Find all sections linked to an entity via GT-Links.

        Looks up the entity in the knowledge graph and returns all tree sections
        where it appears, with summaries and relation context. Narrows retrieval
        scope to entity-relevant sections only. Use after extract_query_entities()
        to see all sections where an entity appears. For text-similarity search
        scoped to a section, use select_by_section() instead.
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved
        return tool_select_by_entity(index, entity_name)

    @mcp.tool()
    def graph_reason(book: str, start_entities: str, hops: int = 2) -> str:
        """Run personalized PageRank over the knowledge graph.

        Builds an adjacency matrix from entity relations and runs PageRank
        starting from the given entities. Maps entity importance scores to
        tree nodes via GT-Links. Returns top-scored nodes and entities.

        Use for structural/relational questions ('how does X connect to Y?').
        For factual questions, search() is usually better. Combine with
        search() via skyline_rank() for multi-signal ranking.

        Args:
            book: Book name
            start_entities: JSON array of entity name strings to start from
            hops: Number of reasoning hops (default 2, more = broader exploration)
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved

        try:
            entities = json.loads(start_entities)
        except json.JSONDecodeError as e:
            return f"Invalid JSON for start_entities: {e}"
        if not isinstance(entities, list):
            return "start_entities must be a JSON array of entity name strings."

        return tool_graph_reason(index, entities, hops)

    @mcp.tool()
    def skyline_rank(query: str, book: str, start_entities: str, hops: int = 2) -> str:
        """Skyline ranking: Pareto frontier over text and graph scores.

        Combines semantic search (S_T) with personalized PageRank (S_G) scores,
        normalizes both to [0,1], and returns the Pareto-optimal (non-dominated)
        set of nodes sorted by combined score. Advanced: use when you need
        explicit control over start entities and hops. search() already does
        skyline ranking automatically when a KG exists — use this only when
        you want to override the auto-extracted entities.

        Args:
            query: Search query for text scoring
            book: Book name
            start_entities: JSON array of entity name strings for graph scoring
            hops: Number of reasoning hops (default 2)
        """
        resolved = _resolve_book(books, book)
        if isinstance(resolved, str):
            return resolved
        _, index = resolved

        try:
            entities = json.loads(start_entities)
        except json.JSONDecodeError as e:
            return f"Invalid JSON for start_entities: {e}"
        if not isinstance(entities, list):
            return "start_entities must be a JSON array of entity name strings."

        return tool_skyline_rank(index, query, entities, hops)

    mcp.run()


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def main() -> None:
    parser = argparse.ArgumentParser(description="BookRAG — semantic book search via MCP")
    sub = parser.add_subparsers(dest="command", required=True)

    idx = sub.add_parser("index", help="Index a book from boko sections JSON (stdin)")
    idx.add_argument("--name", required=True, help="Index name (stored at $XDG_DATA_HOME/bookrag/<name>/)")
    idx.add_argument(
        "--no-llm", action="store_true",
        help="Skip Claude summarization (uses extractive summaries)",
    )

    sub.add_parser("serve", help="Start multi-book MCP server")

    args = parser.parse_args()

    if args.command == "index":
        raw = sys.stdin.read()
        tree = json.loads(raw)
        asyncio.run(build_index(tree, args.name, use_llm=not args.no_llm))
    elif args.command == "serve":
        run_server()


if __name__ == "__main__":
    main()
