-- Keep stable worker IDs and all task foreign keys. The existing unique address
-- column stores canonical endpoint URIs; generated fields expose transport metadata.
ALTER TABLE workers ADD COLUMN transport TEXT GENERATED ALWAYS AS
    (CASE WHEN substr(api_url, 1, 7) = 'iroh://' THEN 'iroh' ELSE 'http' END) VIRTUAL;
ALTER TABLE workers ADD COLUMN endpoint_id TEXT GENERATED ALWAYS AS
    (CASE WHEN substr(api_url, 1, 7) = 'iroh://' THEN substr(api_url, 8, length(api_url) - 8) ELSE NULL END) VIRTUAL;
CREATE UNIQUE INDEX workers_iroh_endpoint_id ON workers(endpoint_id) WHERE endpoint_id IS NOT NULL;
