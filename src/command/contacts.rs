//! `brain contacts` — dispatch for the local contacts book.

use anyhow::{Result, bail};
use chrono::Local;

use crate::cli::{ContactFieldArgs, ContactsAction, ContactsArgs};
use crate::contacts::{self, Fields, model::Contact, render};
use crate::workspace::CommandContext;

impl From<&ContactFieldArgs> for Fields {
    fn from(args: &ContactFieldArgs) -> Self {
        Self {
            name: args.name.clone(),
            job: args.job.clone(),
            company: args.company.clone(),
            email: args.email.clone(),
            phone: args.phone.clone(),
            preferred_comms: args.preferred_comms.clone(),
            address: args.address.clone(),
            tags: args.tags.clone(),
            birthday: args.birthday.clone(),
            notes: args.notes.clone(),
        }
    }
}

fn emit(contacts: &[Contact], pretty: bool) -> Result<()> {
    if pretty {
        print!("{}", render::table(contacts));
    } else {
        println!("{}", serde_json::to_string_pretty(contacts)?);
    }
    Ok(())
}

fn emit_mutation(mutation: &contacts::Mutation) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "action": mutation.action,
            "contact": mutation.contact,
        }))?
    );
    Ok(())
}

pub fn run(args: &ContactsArgs, context: &CommandContext) -> Result<()> {
    let root = context.workspace.root();
    let today = Local::now().date_naive();
    match &args.action {
        None => emit(&contacts::list(root, None, None)?, args.pretty),
        Some(ContactsAction::Add(fields)) => {
            emit_mutation(&contacts::add(root, &Fields::from(fields), today)?)
        }
        Some(ContactsAction::Edit(edit)) => emit_mutation(&contacts::edit(
            root,
            &edit.ident,
            &Fields::from(&edit.fields),
            today,
        )?),
        Some(ContactsAction::Delete(ident)) => {
            emit_mutation(&contacts::delete(root, &ident.ident)?)
        }
        Some(ContactsAction::Get(ident)) => {
            emit(&[contacts::get(root, &ident.ident)?], ident.pretty)
        }
        Some(ContactsAction::List(list)) => emit(
            &contacts::list(root, list.tag.as_deref(), list.job.as_deref())?,
            list.pretty,
        ),
        Some(ContactsAction::Find(find)) => {
            if let Some(field) = &find.field {
                if !crate::contacts::model::SEARCH_FIELDS.contains(&field.as_str()) {
                    bail!(
                        "--field must be one of {}",
                        crate::contacts::model::SEARCH_FIELDS.join(", ")
                    );
                }
            }
            emit(
                &contacts::search(root, &find.query, find.field.as_deref())?,
                find.pretty,
            )
        }
        Some(ContactsAction::Fallback) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&contacts::fallback(root)?)?
            );
            Ok(())
        }
    }
}
