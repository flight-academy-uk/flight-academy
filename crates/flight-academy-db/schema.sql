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


