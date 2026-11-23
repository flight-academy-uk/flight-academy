-- audit_chain_heads: one row per audit chain, holding the chain's tip.
--
-- An append locks its chain's row (SELECT ... FOR UPDATE) at READ COMMITTED,
-- reads the tip from it, inserts the audit row and advances the head before
-- commit. Concurrent appends to one chain therefore queue on that row in
-- order, instead of failing with serialization errors, and a chain cannot
-- fork: the tip is whatever the previous lock holder committed, not the
-- newest row by timestamp. Appends to different chains never wait for each
-- other, and finding the tip is a key lookup rather than a read across every
-- audit_events partition.
--
-- Assumes no writer from before this migration is still appending. Such a
-- writer would insert into audit_events without advancing the head, and the
-- next append would link to a stale tip and fork the chain. That holds here
-- because no release has shipped the previous writer; a future change to
-- how chains are appended must not rely on it while old and new versions
-- run side by side.
--
-- `seq` counts the rows appended to the chain; `tip_hash` is the newest
-- row's payload_hash and `last_occurred_at` its occurred_at. A head with
-- seq = 0 has no rows yet.
CREATE TABLE audit_chain_heads (
    chain_kind       text        NOT NULL CHECK (chain_kind IN ('tenant', 'user', 'platform')),
    chain_id         uuid,
    seq              bigint      NOT NULL DEFAULT 0 CHECK (seq >= 0),
    tip_hash         bytea,
    last_occurred_at timestamptz,
    CONSTRAINT audit_chain_heads_chain_key UNIQUE NULLS NOT DISTINCT (chain_kind, chain_id),
    CONSTRAINT audit_chain_heads_empty CHECK ((seq = 0) = (tip_hash IS NULL)),
    -- The platform plane has a single chain with no id; every other chain
    -- is identified by its tenant or user id.
    CONSTRAINT audit_chain_heads_platform_id CHECK ((chain_kind = 'platform') = (chain_id IS NULL))
);

-- RLS on with no policy: no role that is subject to RLS can read or write
-- a head. The audit writer runs on the pool's session role, which must
-- bypass RLS (checked at startup), exactly as it must for the tip it
-- previously read from audit_events.
ALTER TABLE audit_chain_heads ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_chain_heads FORCE ROW LEVEL SECURITY;

-- Heads for chains that already have rows, taken from each chain's newest
-- row in the order the previous writer used.
INSERT INTO audit_chain_heads (chain_kind, chain_id, seq, tip_hash, last_occurred_at)
SELECT DISTINCT ON (chain_kind, chain_id)
       chain_kind,
       chain_id,
       count(*) OVER (PARTITION BY chain_kind, chain_id),
       payload_hash,
       occurred_at
  FROM audit_events
 ORDER BY chain_kind, chain_id, occurred_at DESC, id DESC;
