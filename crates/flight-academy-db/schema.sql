--
-- PostgreSQL database dump
--



SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
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
);


--
-- Name: audit_events_2026_07; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.audit_events_2026_07 (
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
-- Name: _sqlx_migrations _sqlx_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public._sqlx_migrations
    ADD CONSTRAINT _sqlx_migrations_pkey PRIMARY KEY (version);


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
-- Name: audit_events audit_events_no_delete; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_delete BEFORE DELETE ON public.audit_events FOR EACH ROW EXECUTE FUNCTION public.audit_events_immutable();


--
-- Name: audit_events audit_events_no_truncate; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_truncate BEFORE TRUNCATE ON public.audit_events FOR EACH STATEMENT EXECUTE FUNCTION public.audit_events_immutable();


--
-- Name: audit_events audit_events_no_update; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER audit_events_no_update BEFORE UPDATE ON public.audit_events FOR EACH ROW EXECUTE FUNCTION public.audit_events_immutable();


--
-- PostgreSQL database dump complete
--


