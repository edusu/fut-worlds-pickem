//! Adapter that converts upstream football-data.org payloads into our
//! domain types and emits the appropriate NATS events. Lives here (not in
//! `crates/sports-client`) because the mapping is a service-local concern.

// TODO: implement the DTO -> domain mapping and upsert flow.
