--
-- PostgreSQL database dump
--



SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: pgcrypto; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public;


--
-- Name: EXTENSION pgcrypto; Type: COMMENT; Schema: -; Owner: -
--

COMMENT ON EXTENSION pgcrypto IS 'cryptographic functions';


--
-- Name: audit_events_immutable(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.audit_events_immutable() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    RAISE EXCEPTION 'audit_events is INSERT-only (ADR-009 §A)';
END;
$$;


--
-- Name: tenants_bump_updated_at(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.tenants_bump_updated_at() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: _sqlx_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public._sqlx_migrations (
    version bigint NOT NULL,
    description text NOT NULL,
    installed_on timestamp with time zone DEFAULT now() NOT NULL,
    success boolean NOT NULL,
    checksum bytea NOT NULL,
    execution_time bigint NOT NULL
);


--
-- Name: audit_chain_heads; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_chain_heads (
    chain_kind text NOT NULL,
    chain_id uuid,
    seq bigint DEFAULT 0 NOT NULL,
    tip_hash bytea,
    last_occurred_at timestamp with time zone,
    CONSTRAINT audit_chain_heads_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text]))),
    CONSTRAINT audit_chain_heads_empty CHECK (((seq = 0) = (tip_hash IS NULL))),
    CONSTRAINT audit_chain_heads_platform_id CHECK (((chain_kind = 'platform'::text) = (chain_id IS NULL))),
    CONSTRAINT audit_chain_heads_seq_check CHECK ((seq >= 0))
);

ALTER TABLE ONLY public.audit_chain_heads FORCE ROW LEVEL SECURITY;


--
-- Name: audit_events; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() NOT NULL,
    actor_class text NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb NOT NULL,
    payload_hash bytea NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
)
PARTITION BY RANGE (occurred_at);


--
-- Name: audit_events_2026_06; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_06 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_07; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_07 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_08; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_08 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_09; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_09 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_10; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_10 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_11; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_11 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2026_12; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_12 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_01; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_01 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_02; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_02 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_03; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_03 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_04; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_04 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_05; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_05 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_06; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_06 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_07; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_07 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_08; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_08 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_09; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_09 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_10; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_10 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_11; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_11 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: audit_events_2027_12; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2027_12 (
    id uuid DEFAULT gen_random_uuid() CONSTRAINT audit_events_id_not_null NOT NULL,
    occurred_at timestamp with time zone DEFAULT now() CONSTRAINT audit_events_occurred_at_not_null NOT NULL,
    actor_class text CONSTRAINT audit_events_actor_class_not_null NOT NULL,
    actor_id uuid,
    tenant_id uuid,
    chain_kind text CONSTRAINT audit_events_chain_kind_not_null NOT NULL,
    chain_id uuid,
    prev_hash bytea,
    payload jsonb CONSTRAINT audit_events_payload_not_null NOT NULL,
    payload_hash bytea CONSTRAINT audit_events_payload_hash_not_null NOT NULL,
    CONSTRAINT audit_events_actor_class_check CHECK ((actor_class = ANY (ARRAY['member'::text, 'staff'::text, 'system'::text]))),
    CONSTRAINT audit_events_chain_kind_check CHECK ((chain_kind = ANY (ARRAY['tenant'::text, 'user'::text, 'platform'::text])))
);


--
-- Name: tenant_dek_wrappings; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tenant_dek_wrappings (
    tenant_id uuid NOT NULL,
    record_kind text NOT NULL,
    dek_version integer NOT NULL,
    wrapped_bytes bytea NOT NULL,
    wrap_algo_id smallint NOT NULL,
    kek_id text NOT NULL,
    state text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    retired_at timestamp with time zone,
    CONSTRAINT tenant_dek_wrappings_dek_version_check CHECK ((dek_version >= 1)),
    CONSTRAINT tenant_dek_wrappings_kek_id_check CHECK (((length(kek_id) >= 1) AND (length(kek_id) <= 200))),
    CONSTRAINT tenant_dek_wrappings_record_kind_check CHECK (((length(record_kind) >= 1) AND (length(record_kind) <= 64))),
    CONSTRAINT tenant_dek_wrappings_state_check CHECK ((state = ANY (ARRAY['active'::text, 'retired'::text]))),
    CONSTRAINT tenant_dek_wrappings_state_consistency CHECK ((((state = 'active'::text) AND (retired_at IS NULL)) OR ((state = 'retired'::text) AND (retired_at IS NOT NULL)))),
    CONSTRAINT tenant_dek_wrappings_wrap_algo_id_check CHECK (((wrap_algo_id >= 1) AND (wrap_algo_id <= 254))),
    CONSTRAINT tenant_dek_wrappings_wrapped_bytes_check CHECK (((octet_length(wrapped_bytes) >= 32) AND (octet_length(wrapped_bytes) <= 1024)))
);


--
-- Name: tenants; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.tenants (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    slug text NOT NULL,
    name text NOT NULL,
    tenant_type text NOT NULL,
    settings jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    deleted_at timestamp with time zone,
    deletion_reason text,
    CONSTRAINT tenants_deletion_consistency CHECK ((((deleted_at IS NULL) AND (deletion_reason IS NULL)) OR ((deleted_at IS NOT NULL) AND (deletion_reason IS NOT NULL)))),
    CONSTRAINT tenants_name_check CHECK (((length(name) >= 1) AND (length(name) <= 200))),
    CONSTRAINT tenants_slug_check CHECK ((slug ~ '^[a-z][a-z0-9-]{1,62}$'::text)),
    CONSTRAINT tenants_tenant_type_check CHECK ((tenant_type = ANY (ARRAY['ato'::text, 'part_145'::text, 'airfield_operator'::text])))
);


--
-- Name: audit_events_2026_06; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_06 FOR VALUES FROM ('2026-06-01 00:00:00+00') TO ('2026-07-01 00:00:00+00');


--
-- Name: audit_events_2026_07; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_07 FOR VALUES FROM ('2026-07-01 00:00:00+00') TO ('2026-08-01 00:00:00+00');


--
-- Name: audit_events_2026_08; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_08 FOR VALUES FROM ('2026-08-01 00:00:00+00') TO ('2026-09-01 00:00:00+00');


--
-- Name: audit_events_2026_09; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_09 FOR VALUES FROM ('2026-09-01 00:00:00+00') TO ('2026-10-01 00:00:00+00');


--
-- Name: audit_events_2026_10; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_10 FOR VALUES FROM ('2026-10-01 00:00:00+00') TO ('2026-11-01 00:00:00+00');


--
-- Name: audit_events_2026_11; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_11 FOR VALUES FROM ('2026-11-01 00:00:00+00') TO ('2026-12-01 00:00:00+00');


--
-- Name: audit_events_2026_12; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2026_12 FOR VALUES FROM ('2026-12-01 00:00:00+00') TO ('2027-01-01 00:00:00+00');


--
-- Name: audit_events_2027_01; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_01 FOR VALUES FROM ('2027-01-01 00:00:00+00') TO ('2027-02-01 00:00:00+00');


--
-- Name: audit_events_2027_02; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_02 FOR VALUES FROM ('2027-02-01 00:00:00+00') TO ('2027-03-01 00:00:00+00');


--
-- Name: audit_events_2027_03; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_03 FOR VALUES FROM ('2027-03-01 00:00:00+00') TO ('2027-04-01 00:00:00+00');


--
-- Name: audit_events_2027_04; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_04 FOR VALUES FROM ('2027-04-01 00:00:00+00') TO ('2027-05-01 00:00:00+00');


--
-- Name: audit_events_2027_05; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_05 FOR VALUES FROM ('2027-05-01 00:00:00+00') TO ('2027-06-01 00:00:00+00');


--
-- Name: audit_events_2027_06; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_06 FOR VALUES FROM ('2027-06-01 00:00:00+00') TO ('2027-07-01 00:00:00+00');


--
-- Name: audit_events_2027_07; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_07 FOR VALUES FROM ('2027-07-01 00:00:00+00') TO ('2027-08-01 00:00:00+00');


--
-- Name: audit_events_2027_08; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_08 FOR VALUES FROM ('2027-08-01 00:00:00+00') TO ('2027-09-01 00:00:00+00');


--
-- Name: audit_events_2027_09; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_09 FOR VALUES FROM ('2027-09-01 00:00:00+00') TO ('2027-10-01 00:00:00+00');


--
-- Name: audit_events_2027_10; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_10 FOR VALUES FROM ('2027-10-01 00:00:00+00') TO ('2027-11-01 00:00:00+00');


--
-- Name: audit_events_2027_11; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_11 FOR VALUES FROM ('2027-11-01 00:00:00+00') TO ('2027-12-01 00:00:00+00');


--
-- Name: audit_events_2027_12; Type: TABLE ATTACH; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events ATTACH PARTITION public.audit_events_2027_12 FOR VALUES FROM ('2027-12-01 00:00:00+00') TO ('2028-01-01 00:00:00+00');


--
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


--
-- Name: audit_chain_heads audit_chain_heads_chain_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_chain_heads
    ADD CONSTRAINT audit_chain_heads_chain_key UNIQUE NULLS NOT DISTINCT (chain_kind, chain_id);


--
-- Name: audit_events audit_events_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events
    ADD CONSTRAINT audit_events_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_06 audit_events_2026_06_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_06
    ADD CONSTRAINT audit_events_2026_06_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_07 audit_events_2026_07_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_07
    ADD CONSTRAINT audit_events_2026_07_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_08 audit_events_2026_08_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_08
    ADD CONSTRAINT audit_events_2026_08_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_09 audit_events_2026_09_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_09
    ADD CONSTRAINT audit_events_2026_09_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_10 audit_events_2026_10_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_10
    ADD CONSTRAINT audit_events_2026_10_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_11 audit_events_2026_11_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_11
    ADD CONSTRAINT audit_events_2026_11_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2026_12 audit_events_2026_12_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2026_12
    ADD CONSTRAINT audit_events_2026_12_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_01 audit_events_2027_01_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_01
    ADD CONSTRAINT audit_events_2027_01_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_02 audit_events_2027_02_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_02
    ADD CONSTRAINT audit_events_2027_02_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_03 audit_events_2027_03_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_03
    ADD CONSTRAINT audit_events_2027_03_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_04 audit_events_2027_04_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_04
    ADD CONSTRAINT audit_events_2027_04_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_05 audit_events_2027_05_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_05
    ADD CONSTRAINT audit_events_2027_05_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_06 audit_events_2027_06_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_06
    ADD CONSTRAINT audit_events_2027_06_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_07 audit_events_2027_07_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_07
    ADD CONSTRAINT audit_events_2027_07_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_08 audit_events_2027_08_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_08
    ADD CONSTRAINT audit_events_2027_08_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_09 audit_events_2027_09_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_09
    ADD CONSTRAINT audit_events_2027_09_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_10 audit_events_2027_10_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_10
    ADD CONSTRAINT audit_events_2027_10_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_11 audit_events_2027_11_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_11
    ADD CONSTRAINT audit_events_2027_11_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: audit_events_2027_12 audit_events_2027_12_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.audit_events_2027_12
    ADD CONSTRAINT audit_events_2027_12_pkey PRIMARY KEY (occurred_at, id);


--
-- Name: tenant_dek_wrappings tenant_dek_wrappings_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tenant_dek_wrappings
    ADD CONSTRAINT tenant_dek_wrappings_pkey PRIMARY KEY (tenant_id, record_kind, dek_version);


--
-- Name: tenants tenants_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tenants
    ADD CONSTRAINT tenants_pkey PRIMARY KEY (id);


--
-- Name: audit_events_chain_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_chain_idx ON ONLY public.audit_events USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_06_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_06_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_06 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_07_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_07_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_07 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_08_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_08_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_08 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_09_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_09_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_09 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_10_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_10_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_10 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_11_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_11_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_11 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2026_12_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2026_12_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2026_12 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_01_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_01_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_01 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_02_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_02_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_02 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_03_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_03_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_03 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_04_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_04_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_04 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_05_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_05_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_05 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_06_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_06_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_06 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_07_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_07_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_07 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_08_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_08_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_08 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_09_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_09_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_09 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_10_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_10_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_10 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_11_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_11_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_11 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: audit_events_2027_12_chain_kind_chain_id_occurred_at_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX audit_events_2027_12_chain_kind_chain_id_occurred_at_idx ON public.audit_events_2027_12 USING btree (chain_kind, chain_id, occurred_at);


--
-- Name: tenant_dek_wrappings_one_active; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX tenant_dek_wrappings_one_active ON public.tenant_dek_wrappings USING btree (tenant_id, record_kind) WHERE (state = 'active'::text);


--
-- Name: tenants_slug_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX tenants_slug_unique ON public.tenants USING btree (slug) WHERE (deleted_at IS NULL);


--
-- Name: tenants_updated_at_id_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX tenants_updated_at_id_idx ON public.tenants USING btree (updated_at, id);


--
-- Name: audit_events_2026_06_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_06_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_06_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_06_pkey;


--
-- Name: audit_events_2026_07_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_07_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_07_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_07_pkey;


--
-- Name: audit_events_2026_08_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_08_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_08_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_08_pkey;


--
-- Name: audit_events_2026_09_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_09_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_09_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_09_pkey;


--
-- Name: audit_events_2026_10_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_10_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_10_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_10_pkey;


--
-- Name: audit_events_2026_11_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_11_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_11_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_11_pkey;


--
-- Name: audit_events_2026_12_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2026_12_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2026_12_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2026_12_pkey;


--
-- Name: audit_events_2027_01_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_01_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_01_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_01_pkey;


--
-- Name: audit_events_2027_02_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_02_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_02_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_02_pkey;


--
-- Name: audit_events_2027_03_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_03_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_03_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_03_pkey;


--
-- Name: audit_events_2027_04_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_04_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_04_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_04_pkey;


--
-- Name: audit_events_2027_05_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_05_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_05_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_05_pkey;


--
-- Name: audit_events_2027_06_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_06_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_06_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_06_pkey;


--
-- Name: audit_events_2027_07_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_07_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_07_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_07_pkey;


--
-- Name: audit_events_2027_08_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_08_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_08_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_08_pkey;


--
-- Name: audit_events_2027_09_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_09_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_09_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_09_pkey;


--
-- Name: audit_events_2027_10_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_10_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_10_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_10_pkey;


--
-- Name: audit_events_2027_11_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_11_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_11_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_11_pkey;


--
-- Name: audit_events_2027_12_chain_kind_chain_id_occurred_at_idx; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_chain_idx ATTACH PARTITION public.audit_events_2027_12_chain_kind_chain_id_occurred_at_idx;


--
-- Name: audit_events_2027_12_pkey; Type: INDEX ATTACH; Schema: public; Owner: -
--

ALTER INDEX public.audit_events_pkey ATTACH PARTITION public.audit_events_2027_12_pkey;


--
-- Name: audit_events audit_events_no_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_delete BEFORE DELETE ON public.audit_events FOR EACH STATEMENT EXECUTE FUNCTION public.audit_events_immutable();


--
-- Name: audit_events audit_events_no_truncate; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_truncate BEFORE TRUNCATE ON public.audit_events FOR EACH STATEMENT EXECUTE FUNCTION public.audit_events_immutable();


--
-- Name: audit_events audit_events_no_update; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_update BEFORE UPDATE ON public.audit_events FOR EACH STATEMENT EXECUTE FUNCTION public.audit_events_immutable();


--
-- Name: tenants tenants_bump_updated_at; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER tenants_bump_updated_at BEFORE UPDATE ON public.tenants FOR EACH ROW EXECUTE FUNCTION public.tenants_bump_updated_at();


--
-- Name: tenant_dek_wrappings tenant_dek_wrappings_tenant_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.tenant_dek_wrappings
    ADD CONSTRAINT tenant_dek_wrappings_tenant_id_fkey FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE;


--
-- Name: audit_chain_heads; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.audit_chain_heads ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_events; Type: ROW SECURITY; Schema: public; Owner: -
--

ALTER TABLE public.audit_events ENABLE ROW LEVEL SECURITY;

--
-- Name: audit_events audit_events_tenant_isolation; Type: POLICY; Schema: public; Owner: -
--

CREATE POLICY audit_events_tenant_isolation ON public.audit_events FOR SELECT TO app_api USING (((chain_kind = 'tenant'::text) AND (chain_id = (NULLIF(current_setting('app.current_tenant'::text, true), ''::text))::uuid)));


--
-- PostgreSQL database dump complete
--


