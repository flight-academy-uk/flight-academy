-- Monthly audit_events partitions, August 2026 to December 2027.
--
-- The init migration created partitions for June and July 2026 only, and
-- audit_events has no DEFAULT partition. From 2026-08-01 every insert
-- failed with "no partition of relation found for row", so every audited
-- write (tenant PATCH and DELETE) was rolled back with a 500.
--
-- This migration restores headroom with plain partitions. It deliberately
-- adds no DEFAULT partition: a row landing there would block the later
-- creation of the partition covering its month. Creating partitions ahead
-- of time automatically (ADR-009 §E) is a separate change; until it
-- exists, a migration must extend this range before 2028-01-01.

CREATE TABLE audit_events_2026_08 PARTITION OF audit_events
    FOR VALUES FROM ('2026-08-01') TO ('2026-09-01');

CREATE TABLE audit_events_2026_09 PARTITION OF audit_events
    FOR VALUES FROM ('2026-09-01') TO ('2026-10-01');

CREATE TABLE audit_events_2026_10 PARTITION OF audit_events
    FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');

CREATE TABLE audit_events_2026_11 PARTITION OF audit_events
    FOR VALUES FROM ('2026-11-01') TO ('2026-12-01');

CREATE TABLE audit_events_2026_12 PARTITION OF audit_events
    FOR VALUES FROM ('2026-12-01') TO ('2027-01-01');

CREATE TABLE audit_events_2027_01 PARTITION OF audit_events
    FOR VALUES FROM ('2027-01-01') TO ('2027-02-01');

CREATE TABLE audit_events_2027_02 PARTITION OF audit_events
    FOR VALUES FROM ('2027-02-01') TO ('2027-03-01');

CREATE TABLE audit_events_2027_03 PARTITION OF audit_events
    FOR VALUES FROM ('2027-03-01') TO ('2027-04-01');

CREATE TABLE audit_events_2027_04 PARTITION OF audit_events
    FOR VALUES FROM ('2027-04-01') TO ('2027-05-01');

CREATE TABLE audit_events_2027_05 PARTITION OF audit_events
    FOR VALUES FROM ('2027-05-01') TO ('2027-06-01');

CREATE TABLE audit_events_2027_06 PARTITION OF audit_events
    FOR VALUES FROM ('2027-06-01') TO ('2027-07-01');

CREATE TABLE audit_events_2027_07 PARTITION OF audit_events
    FOR VALUES FROM ('2027-07-01') TO ('2027-08-01');

CREATE TABLE audit_events_2027_08 PARTITION OF audit_events
    FOR VALUES FROM ('2027-08-01') TO ('2027-09-01');

CREATE TABLE audit_events_2027_09 PARTITION OF audit_events
    FOR VALUES FROM ('2027-09-01') TO ('2027-10-01');

CREATE TABLE audit_events_2027_10 PARTITION OF audit_events
    FOR VALUES FROM ('2027-10-01') TO ('2027-11-01');

CREATE TABLE audit_events_2027_11 PARTITION OF audit_events
    FOR VALUES FROM ('2027-11-01') TO ('2027-12-01');

CREATE TABLE audit_events_2027_12 PARTITION OF audit_events
    FOR VALUES FROM ('2027-12-01') TO ('2028-01-01');
