ALTER TABLE listings ADD COLUMN phash TEXT;
CREATE INDEX idx_listings_phash ON listings (phash) WHERE phash IS NOT NULL;

-- NULL = use the global poll_interval_minutes setting.
ALTER TABLE searches ADD COLUMN poll_interval_minutes INTEGER;
