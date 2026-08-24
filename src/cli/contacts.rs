//! `brain contacts` — the local contacts book's CLI surface.

use clap::{Args, Subcommand};

#[derive(Args, Debug)]
pub struct ContactsArgs {
    #[command(subcommand)]
    pub action: Option<ContactsAction>,

    /// Print a table instead of JSON. Bare `brain contacts` lists everyone.
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Subcommand, Debug)]
pub enum ContactsAction {
    /// Add a contact. `--name` is required; every other field is optional.
    Add(ContactFieldArgs),

    /// Edit a contact by id (`C003`) or name. Only the fields you pass change.
    Edit(ContactEditArgs),

    /// Delete a contact by id or name.
    Delete(ContactIdentArgs),

    /// List contacts, optionally narrowed by tag or job.
    List(ContactListArgs),

    /// Search contacts. Every searched field by default, or one with `--field`.
    Find(ContactFindArgs),

    /// Show one contact by id or name.
    Get(ContactIdentArgs),

    /// Print the configured external fallback directory, when one is set.
    Fallback,
}

#[derive(Args, Debug, Default)]
pub struct ContactFieldArgs {
    /// Full name.
    #[arg(long)]
    pub name: Option<String>,
    /// Role or job, e.g. "Accountant".
    #[arg(long)]
    pub job: Option<String>,
    #[arg(long)]
    pub company: Option<String>,
    #[arg(long)]
    pub email: Option<String>,
    /// Phone or WhatsApp number.
    #[arg(long)]
    pub phone: Option<String>,
    /// One of email, whatsapp, phone.
    #[arg(long)]
    pub preferred_comms: Option<String>,
    #[arg(long)]
    pub address: Option<String>,
    /// Semicolon-separated tags, e.g. "family;medical".
    #[arg(long)]
    pub tags: Option<String>,
    /// Birthday, YYYY-MM-DD.
    #[arg(long)]
    pub birthday: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Args, Debug)]
pub struct ContactEditArgs {
    /// Contact id (`C003`) or name.
    pub ident: String,

    #[command(flatten)]
    pub fields: ContactFieldArgs,
}

#[derive(Args, Debug)]
pub struct ContactIdentArgs {
    /// Contact id (`C003`) or name.
    pub ident: String,

    /// Print a table instead of JSON.
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Args, Debug)]
pub struct ContactListArgs {
    /// Only contacts carrying this tag.
    #[arg(long)]
    pub tag: Option<String>,

    /// Only contacts whose job contains this.
    #[arg(long)]
    pub job: Option<String>,

    /// Print a table instead of JSON.
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Args, Debug)]
pub struct ContactFindArgs {
    /// Text to search for.
    pub query: String,

    /// Restrict the search to one field.
    #[arg(long)]
    pub field: Option<String>,

    /// Print a table instead of JSON.
    #[arg(long)]
    pub pretty: bool,
}
