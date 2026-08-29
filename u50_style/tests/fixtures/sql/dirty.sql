CREATE TABLE "public"."page_nimbus" (id bigint PRIMARY KEY GENERATED always AS IDENTITY,
                                                                     PATH text NOT NULL UNIQUE,
                                                                                        checksum text, meta JSONB,
                                                                                                            TYPE text, SOURCE text, content text, VERSION UUID,
                                                                                                                                                          last_refresh timestamptz,
                                                                                                                                                          fts_tokens
                                     TSVECTOR GENERATED always AS (to_tsvector('english', content)) stored,
                                                        title_tokens
                                     TSVECTOR GENERATED always AS (to_tsvector('english', coalesce(meta ->> 'title', ''))) stored);

ALTER TABLE "public"."page_nimbus" ENABLE ROW LEVEL SECURITY;

CREATE policy "anon can read page_nimbus" ON public.page_nimbus
FOR
SELECT TO anon USING (TRUE);

CREATE policy "authenticated can read page_nimbus" ON public.page_nimbus
FOR
SELECT TO authenticated USING (TRUE);

CREATE TABLE "public"."page_section_nimbus"
    (id bigint PRIMARY KEY GENERATED always AS IDENTITY,
                                     page_id bigint NOT NULL REFERENCES public.page_nimbus (id) ON DELETE CASCADE,
                                                                                                          content text, token_count int, embedding vector(1536),
                                                                                                                                                   slug text, heading text, rag_ignore boolean DEFAULT FALSE);

ALTER TABLE "public"."page_section_nimbus" ENABLE ROW LEVEL SECURITY;

CREATE policy "anon can read page_section_nimbus" ON public.page_section_nimbus
FOR
SELECT TO anon USING (TRUE);

CREATE policy "authenticated can read page_section_nimbus" ON public.page_section_nimbus
FOR
SELECT TO authenticated USING (TRUE);

CREATE INDEX fts_search_index_content_nimbus ON page_nimbus USING gin(fts_tokens);

CREATE INDEX fts_search_index_title_nimbus ON page_nimbus USING gin(title_tokens);

CREATE OR REPLACE FUNCTION docs_search_fts_nimbus(query text) RETURNS TABLE (id bigint, PATH text, TYPE text, title text, subtitle text, description text)
SET search_path = '' LANGUAGE PLPGSQL AS $$ #variable_conflict use_variable begin return query select page_nimbus.id, page_nimbus.path, page_nimbus.type, page_nimbus.meta ->> 'title' as title, page_nimbus.meta ->> 'subtitle' as subtitle, page_nimbus.meta ->> 'description' as description from public.page_nimbus where title_tokens @@ websearch_to_tsquery(query) or fts_tokens @@ websearch_to_tsquery(query) order by greatest( least(10 * ts_rank(title_tokens, websearch_to_tsquery(query)), 1), ts_rank(fts_tokens, websearch_to_tsquery(query)) ) desc limit 10; end; $$;

CREATE OR REPLACE FUNCTION match_embedding_nimbus(embedding vector(1536), match_threshold float DEFAULT 0.78, max_results int DEFAULT 30) RETURNS
SETOF public.page_section_nimbus
SET search_path = '' LANGUAGE PLPGSQL AS $$ #variable_conflict use_variable begin return query select * from public.page_section_nimbus where (page_section_nimbus.embedding operator(public.<#>) embedding) <= -match_threshold order by page_section_nimbus.embedding operator(public.<#>) embedding limit max_results; end; $$;

CREATE OR REPLACE FUNCTION search_content_hybrid_nimbus(query_text text, query_embedding vector(1536), max_result int DEFAULT 30, full_text_weight float DEFAULT 1, semantic_weight float DEFAULT 1, rrf_k int DEFAULT 50, match_threshold float DEFAULT 0.78, include_full_content boolean DEFAULT FALSE) RETURNS TABLE (id bigint, page_title text, TYPE text, href text, content text, metadata JSON,
                                                                                                                                                                                                                                                                                                                                                                                                   subsections JSON[]) LANGUAGE SQL
SET search_path = '' AS $$ with full_text as ( select id, row_number() over(order by greatest( least(10 * ts_rank(title_tokens, websearch_to_tsquery(query_text)), 1), ts_rank(fts_tokens, websearch_to_tsquery(query_text)) ) desc) as rank_ix from public.page_nimbus where title_tokens @@ websearch_to_tsquery(query_text) or fts_tokens @@ websearch_to_tsquery(query_text) order by rank_ix limit least(max_result, 30) * 2 ), semantic as ( select page_id as id, row_number() over () as rank_ix from public.match_embedding_nimbus(query_embedding, match_threshold, max_result * 2) ), rrf as ( select coalesce(full_text.id, semantic.id) as id, coalesce(1.0 / (rrf_k + full_text.rank_ix), 0.0) * full_text_weight + coalesce(1.0 / (rrf_k + semantic.rank_ix), 0.0) * semantic_weight as rrf_score from full_text full outer join semantic on full_text.id = semantic.id ) select page_nimbus.id, page_nimbus.meta ->> 'title' as page_title, page_nimbus.type, public.get_full_content_url(page_nimbus.type, page_nimbus.path, null) as href, case when include_full_content then page_nimbus.content else null end as content, page_nimbus.meta as metadata, array_agg(json_build_object( 'title', page_section_nimbus.heading, 'href', public.get_full_content_url(page_nimbus.type, page_nimbus.path, page_section_nimbus.slug), 'content', page_section_nimbus.content )) as subsections from rrf join public.page_nimbus on page_nimbus.id = rrf.id left join public.page_section_nimbus on page_section_nimbus.page_id = page_nimbus.id where rrf.rrf_score > 0 group by page_nimbus.id order by max(rrf.rrf_score) desc limit max_result; $$;

CREATE OR REPLACE FUNCTION match_page_sections_v2_nimbus(embedding vector(1536), match_threshold float, min_content_length int) RETURNS
SETOF page_section_nimbus
SET search_path = '' LANGUAGE PLPGSQL AS $$ #variable_conflict use_variable begin return query select * from public.page_section_nimbus where length(page_section_nimbus.content) >= min_content_length and (page_section_nimbus.embedding operator(public.<#>) embedding) * -1 > match_threshold order by page_section_nimbus.embedding operator(public.<#>) embedding; end; $$;

CREATE OR REPLACE FUNCTION docs_search_embeddings_nimbus(embedding vector(1536), match_threshold float) RETURNS TABLE (id bigint, PATH text, TYPE text, title text, subtitle text, description text, headings text[], slugs text[])
SET search_path = '' LANGUAGE PLPGSQL AS $$ #variable_conflict use_variable begin return query with match as( select * from public.page_section_nimbus where (page_section_nimbus.embedding operator(public.<#>) embedding) * -1 > match_threshold order by page_section_nimbus.embedding operator(public.<#>) embedding limit 10 ) select page_nimbus.id, page_nimbus.path, page_nimbus.type, page_nimbus.meta ->> 'title' as title, page_nimbus.meta ->> 'subtitle' as title, page_nimbus.meta ->> 'description' as description, array_agg(match.heading) as headings, array_agg(match.slug) as slugs from public.page_nimbus join match on match.page_id = page_nimbus.id group by page_nimbus.id; end; $$;

CREATE OR REPLACE FUNCTION search_content_nimbus(embedding vector(1536), include_full_content boolean DEFAULT FALSE, match_threshold float DEFAULT 0.78, max_result int DEFAULT 30) RETURNS TABLE (id bigint, page_title text, TYPE text, href text, content text, metadata JSON,
                                                                                                                                                                                                                                                                            subsections JSON[])
SET search_path = '' LANGUAGE SQL AS $$ with matched_section as ( select *, row_number() over () as ranking from public.match_embedding_nimbus( embedding, match_threshold, max_result ) ) select page_nimbus.id, meta ->> 'title' as page_title, type, public.get_full_content_url(type, path, null) as href, case when include_full_content then page_nimbus.content else null end as content, meta as metadata, array_agg( json_build_object( 'title', heading, 'href', public.get_full_content_url(type, path, slug), 'content', matched_section.content ) ) from matched_section join public.page_nimbus on matched_section.page_id = page_nimbus.id group by page_nimbus.id order by min(ranking); $$;

CREATE TABLE "public"."active_pgbouncer_projects" ("id" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,
                                                                                                "project_ref" text);

ALTER TABLE "public"."active_pgbouncer_projects" ENABLE ROW LEVEL SECURITY;

CREATE TABLE "public"."vercel_project_connections_without_supavisor" ("id" bigint GENERATED BY DEFAULT AS IDENTITY NOT NULL,
                                                                                                                   "project_ref" text NOT NULL);

ALTER TABLE "public"."vercel_project_connections_without_supavisor" ENABLE ROW LEVEL SECURITY;

CREATE UNIQUE INDEX active_pgbouncer_projects_pkey ON public.active_pgbouncer_projects USING btree (id);

CREATE UNIQUE INDEX vercel_project_connections_without_supavisor_pkey ON public.vercel_project_connections_without_supavisor USING btree (id);

ALTER TABLE "public"."active_pgbouncer_projects" ADD CONSTRAINT "active_pgbouncer_projects_pkey" PRIMARY KEY USING INDEX "active_pgbouncer_projects_pkey";

ALTER TABLE "public"."vercel_project_connections_without_supavisor" ADD CONSTRAINT "vercel_project_connections_without_supavisor_pkey" PRIMARY KEY USING INDEX "vercel_project_connections_without_supavisor_pkey";

GRANT
DELETE ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT
INSERT ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT REFERENCES ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT
SELECT ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT TRIGGER ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT
TRUNCATE ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT
UPDATE ON TABLE "public"."active_pgbouncer_projects" TO "anon";

GRANT
DELETE ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT
INSERT ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT REFERENCES ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT
SELECT ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT TRIGGER ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT
TRUNCATE ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT
UPDATE ON TABLE "public"."active_pgbouncer_projects" TO "authenticated";

GRANT
DELETE ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT
INSERT ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT REFERENCES ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT
SELECT ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT TRIGGER ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT
TRUNCATE ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT
UPDATE ON TABLE "public"."active_pgbouncer_projects" TO "service_role";

GRANT
DELETE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT
INSERT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT REFERENCES ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT
SELECT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT TRIGGER ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT
TRUNCATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT
UPDATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "anon";

GRANT
DELETE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT
INSERT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT REFERENCES ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT
SELECT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT TRIGGER ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT
TRUNCATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT
UPDATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "authenticated";

GRANT
DELETE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT
INSERT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT REFERENCES ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT
SELECT ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT TRIGGER ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT
TRUNCATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

GRANT
UPDATE ON TABLE "public"."vercel_project_connections_without_supavisor" TO "service_role";

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE public.launch_weeks (id text NOT NULL PRIMARY KEY,
                                                   created_at timestamp WITH TIME ZONE NOT NULL DEFAULT timezone ('utc'::text, now()),
                                                                                                        start_date timestamp WITH TIME ZONE NULL,
                                                                                                                                            end_date timestamp WITH TIME ZONE NULL);

ALTER TABLE public.launch_weeks ENABLE ROW LEVEL SECURITY;

CREATE policy "Allow public read access" ON "public"."launch_weeks" AS PERMISSIVE
FOR
SELECT USING (TRUE);

INSERT INTO public.launch_weeks (id)
VALUES ('lw12');

CREATE TABLE public.tickets (id UUID NOT NULL DEFAULT uuid_generate_v4(),
                                                      created_at timestamp WITH TIME ZONE NOT NULL DEFAULT timezone('utc'::text, now()),
                                                                                                           launch_week text NOT NULL REFERENCES public.launch_weeks (id),
                                                                                                                                                user_id UUID NOT NULL REFERENCES auth.users (id),
                                                                                                                                                                                 email text NULL,
                                                                                                                                                                                            name text NULL,
                                                                                                                                                                                                      username text NULL,
                                                                                                                                                                                                                    referred_by text NULL,
                                                                                                                                                                                                                                     shared_on_twitter timestamp WITH TIME ZONE NULL,
                                                                                                                                                                                                                                                                                shared_on_linkedin timestamp WITH TIME ZONE NULL,
                                                                                                                                                                                                                                                                                                                            game_won_at timestamp WITH TIME ZONE NULL,
                                                                                                                                                                                                                                                                                                                                                                 ticket_number bigint GENERATED BY DEFAULT AS IDENTITY,
                                                                                                                                                                                                                                                                                                                                                                                                              metadata JSONB NULL,
                                                                                                                                                                                                                                                                                                                                                                                                                             ROLE text NULL,
                                                                                                                                                                                                                                                                                                                                                                                                                                       company text NULL,
                                                                                                                                                                                                                                                                                                                                                                                                                                                    LOCATION text NULL,
                                                                                                                                                                                                                                                                                                                                                                                                                                                                  CONSTRAINT tickets_pkey PRIMARY KEY (id), CONSTRAINT tickets_email_key UNIQUE (email,
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 launch_week), CONSTRAINT tickets_ticket_number_key UNIQUE (ticket_number,
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            launch_week), CONSTRAINT tickets_username_key UNIQUE (username,
                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  launch_week), CONSTRAINT public_tickets_id_fkey
                             FOREIGN KEY (user_id) REFERENCES auth.users (id));

ALTER TABLE public.tickets ENABLE ROW LEVEL SECURITY;

ALTER publication supabase_realtime ADD TABLE public.tickets;

GRANT
UPDATE (ROLE) ON TABLE public.tickets TO authenticated;

GRANT
UPDATE (company) ON TABLE public.tickets TO authenticated;

GRANT
UPDATE (LOCATION) ON TABLE public.tickets TO authenticated;

CREATE policy "Allow user to select own ticket" ON public.tickets AS PERMISSIVE
FOR
SELECT TO authenticated USING (user_id = auth.uid());

CREATE policy "Allow authenticated user to update its own ticket" ON public.tickets AS permissive
FOR
UPDATE TO authenticated USING (user_id = auth.uid()) WITH CHECK (user_id = auth.uid());

CREATE policy "Allow insert for authenticated users only" ON public.tickets AS permissive
FOR
INSERT TO authenticated WITH CHECK (user_id = auth.uid());

CREATE OR REPLACE VIEW public.tickets_view WITH (security_invoker=ON) AS WITH lw12_referrals AS
    (SELECT tickets_1.referred_by,
            count(*) AS referrals
     FROM tickets tickets_1
     WHERE tickets_1.referred_by IS NOT NULL
     GROUP BY tickets_1.referred_by)
SELECT tickets.id,
       tickets.name,
       tickets.username,
       tickets.ticket_number,
       tickets.created_at,
       tickets.launch_week,
       tickets.shared_on_twitter,
       tickets.shared_on_linkedin,
       tickets.metadata,
       tickets.role,
       tickets.company,
       tickets.location,
       CASE
           WHEN lw12_referrals.referrals IS NULL THEN 0::bigint
           ELSE lw12_referrals.referrals
       END AS referrals,
       CASE
           WHEN tickets.shared_on_twitter IS NOT NULL
                AND tickets.shared_on_linkedin IS NOT NULL THEN TRUE
           ELSE FALSE
       END AS platinum,
       CASE
           WHEN tickets.game_won_at IS NOT NULL THEN TRUE
           ELSE FALSE
       END AS secret
FROM tickets
LEFT JOIN lw12_referrals ON tickets.username = lw12_referrals.referred_by;

CREATE TABLE public.meetups (id UUID NOT NULL DEFAULT uuid_generate_v4(),
                                                      created_at timestamp WITH TIME ZONE NOT NULL DEFAULT now(),
                                                                                                           launch_week text NOT NULL REFERENCES public.launch_weeks (id),
                                                                                                                                                title text NULL,
                                                                                                                                                           country text NULL,
                                                                                                                                                                        start_at timestamp WITH TIME ZONE NULL,
                                                                                                                                                                                                          LINK text NULL,
                                                                                                                                                                                                                    display_info text NULL,
                                                                                                                                                                                                                                      is_live boolean NOT NULL DEFAULT FALSE,
                                                                                                                                                                                                                                                                       is_published boolean NOT NULL DEFAULT FALSE,
                                                                                                                                                                                                                                                                                                             CONSTRAINT meetups_pkey PRIMARY KEY (id));

ALTER TABLE public.meetups ENABLE ROW LEVEL SECURITY;

ALTER publication supabase_realtime ADD TABLE public.meetups;

CREATE policy "Allow anybody to select all meetups" ON public.meetups AS permissive
FOR
SELECT USING (TRUE);

ALTER FUNCTION public.update_last_changed_checksum
SET search_path = '';

ALTER FUNCTION public.cleanup_last_changed_pages
SET search_path = '';

CREATE OR REPLACE FUNCTION match_page_sections_v2(embedding vector(1536), match_threshold float, min_content_length int) RETURNS
SETOF page_section LANGUAGE PLPGSQL
SET search_path = '' AS $$ #variable_conflict use_variable begin return query select * from public.page_section where length(page_section.content) >= min_content_length and (page_section.embedding operator(public.<#>) embedding) * -1 > match_threshold order by page_section.embedding operator(public.<#>) embedding; end; $$;

CREATE OR REPLACE FUNCTION ipv6_active_status (project_ref text) RETURNS TABLE (pgbouncer_active boolean, vercel_active boolean)
SET search_path = '' AS $$ declare pgbouncer_active boolean; vercel_active boolean; begin select exists ( select 1 from public.active_pgbouncer_projects ap where ap.project_ref = $1 ) into pgbouncer_active; select exists ( select 1 from public.vercel_project_connections_without_supavisor vp where vp.project_ref = $1 ) into vercel_active; return query select pgbouncer_active, vercel_active; end; $$ LANGUAGE PLPGSQL SECURITY DEFINER;

CREATE OR REPLACE FUNCTION docs_search_embeddings(embedding vector(1536), match_threshold float) RETURNS TABLE (id int8, PATH text, TYPE text, title text, subtitle text, description text, headings text[], slugs text[]) LANGUAGE PLPGSQL
SET search_path = '' AS $$ #variable_conflict use_variable begin return query with match as( select * from public.page_section where (page_section.embedding operator(public.<#>) embedding) * -1 > match_threshold order by page_section.embedding operator(public.<#>) embedding limit 10 ) select page.id, page.path, page.type, page.meta ->> 'title' as title, page.meta ->> 'subtitle' as title, page.meta ->> 'description' as description, array_agg(match.heading) as headings, array_agg(match.slug) as slugs from public.page join match on match.page_id = page.id group by page.id; end; $$;

CREATE OR REPLACE FUNCTION docs_search_fts(query text) RETURNS TABLE (id int8, PATH text, TYPE text, title text, subtitle text, description text) LANGUAGE PLPGSQL
SET search_path = '' AS $$ #variable_conflict use_variable begin return query select page.id, page.path, page.type, page.meta ->> 'title' as title, page.meta ->> 'subtitle' as subtitle, page.meta ->> 'description' as description from public.page where title_tokens @@ websearch_to_tsquery(query) or fts_tokens @@ websearch_to_tsquery(query) order by greatest( least(10 * ts_rank(title_tokens, websearch_to_tsquery(query)), 1), ts_rank(fts_tokens, websearch_to_tsquery(query)) ) desc limit 10; end; $$;

DROP FUNCTION public.match_page_sections;

DROP FUNCTION public.get_page_parents;

CREATE SCHEMA IF NOT EXISTS utils;

GRANT USAGE ON SCHEMA utils TO anon;

GRANT USAGE ON SCHEMA utils TO authenticated;

ALTER DEFAULT PRIVILEGES IN SCHEMA utils REVOKE EXECUTE ON functions
FROM anon;

ALTER DEFAULT PRIVILEGES IN SCHEMA utils REVOKE EXECUTE ON functions
FROM authenticated;

CREATE OR REPLACE FUNCTION utils.update_timestamp() RETURNS TRIGGER
SET search_path = '' LANGUAGE PLPGSQL AS $$ begin new.updated_at = now(); return new; end; $$;

GRANT EXECUTE ON FUNCTION utils.update_timestamp() TO anon;

GRANT EXECUTE ON FUNCTION utils.update_timestamp() TO authenticated;

CREATE SCHEMA IF NOT EXISTS content;

GRANT USAGE ON SCHEMA content TO anon;

GRANT USAGE ON SCHEMA content TO authenticated;

CREATE TABLE IF NOT EXISTS content.service (id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
                                                                        name text NOT NULL UNIQUE,
                                                                                           created_at timestamptz DEFAULT now(),
                                                                                                                          updated_at timestamptz DEFAULT now(),
                                                                                                                                                         deleted_at timestamptz DEFAULT NULL);

CREATE OR REPLACE TRIGGER sync_updated_at_content_service
BEFORE
UPDATE ON content.service
FOR EACH ROW EXECUTE FUNCTION utils.update_timestamp();

CREATE OR REPLACE RULE soft_delete_content_service AS ON
DELETE TO content.service DO INSTEAD
    (UPDATE content.service
     SET deleted_at = now()
     WHERE id = old.id);

ALTER TABLE content.service ENABLE ROW LEVEL SECURITY;

CREATE INDEX IF NOT EXISTS idx_content_service_id_nondeleted_only ON content.service (id)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_content_service_name_nondeleted_only ON content.service (name)
WHERE deleted_at IS NULL;

INSERT INTO content.service (name)
VALUES ('AUTH'),
       ('REALTIME'),
       ('STORAGE');

CREATE TABLE IF NOT EXISTS content.error
    (code text NOT NULL,
               service UUID NOT NULL REFERENCES content.service (id) ON DELETE RESTRICT,
                                                                               http_status_code smallint, message text, created_at timestamptz DEFAULT now(),
                                                                                                                                                       updated_at timestamptz DEFAULT now(),
                                                                                                                                                                                      deleted_at timestamptz DEFAULT NULL,
                                                                                                                                                                                                                     PRIMARY KEY (service,
                                                                                                                                                                                                                                  code));

CREATE OR REPLACE TRIGGER sync_updated_at_content_error
BEFORE
UPDATE ON content.error
FOR EACH ROW EXECUTE FUNCTION utils.update_timestamp();

CREATE OR REPLACE RULE soft_delete_content_error AS ON
DELETE TO content.error DO INSTEAD
    (UPDATE content.error
     SET deleted_at = now()
     WHERE code = old.code
         AND service = old.service);

ALTER TABLE content.error ENABLE ROW LEVEL SECURITY;

CREATE INDEX IF NOT EXISTS idx_content_error_service_code_nondeleted_only ON content.error (service, code)
WHERE deleted_at IS NULL;

GRANT
SELECT (id,
        name,
        deleted_at) ON content.service TO anon;

GRANT
SELECT (id,
        name,
        deleted_at) ON content.service TO authenticated;

GRANT
SELECT (code,
        service,
        http_status_code,
        message,
        deleted_at) ON content.error TO anon;

GRANT
SELECT (code,
        service,
        http_status_code,
        message,
        deleted_at) ON content.error TO authenticated;

CREATE policy content_service_anon_select_all ON content.service
FOR
SELECT TO anon USING (deleted_at IS NULL);

CREATE policy content_service_authenticated_select_all ON content.service
FOR
SELECT TO authenticated USING (deleted_at IS NULL);

CREATE policy content_error_anon_select_all ON content.error
FOR
SELECT TO anon USING (deleted_at IS NULL);

CREATE policy content_error_authenticated_select_all ON content.error
FOR
SELECT TO authenticated USING (deleted_at IS NULL);

ALTER TABLE content.error ADD COLUMN metadata JSONB;

ALTER TABLE content.error ADD CONSTRAINT constraint_content_error_metadata_schema CHECK (jsonb_matches_schema('{ "type": "object", "properties": { "references": { "type": "array", "items": { "type": "object", "properties": { "href": { "type": "string" }, "description": { "type": "string" } }, "required": ["href", "description"] } } } }', metadata));

DROP FUNCTION content.update_error_code;

CREATE FUNCTION content.update_error_code(code text, service text, http_status_code smallint DEFAULT NULL, message text DEFAULT NULL, metadata JSONB DEFAULT NULL) RETURNS boolean
SET search_path = '' LANGUAGE PLPGSQL AS $$ #variable_conflict use_variable declare service_id uuid; result boolean; begin insert into content.service (name) values (service) on conflict (name) do nothing; select id into service_id from content.service where name = service; insert into content.error ( service, code, http_status_code, message, metadata ) values (service_id, code, http_status_code, message, metadata) on conflict on constraint error_pkey do update set http_status_code = excluded.http_status_code, message = excluded.message, metadata = excluded.metadata where error.service = service_id and error.code = code and ( error.http_status_code is distinct from excluded.http_status_code or error.message is distinct from excluded.message or error.metadata is distinct from excluded.metadata ) returning true into result; return coalesce(result, false); end; $$;
