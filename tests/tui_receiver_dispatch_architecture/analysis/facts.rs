#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) enum TypeShape {
    #[default]
    Opaque,
    Tuple(Vec<TypeFact>),
    Sequence(Box<TypeFact>),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TypeFact {
    pub(super) canonical: Option<String>,
    pub(super) borrowed: bool,
    pub(super) unresolved_glob: bool,
    pub(super) inbound_job: bool,
    pub(super) agent_controller: bool,
    pub(super) app: bool,
    pub(super) brain_panel: bool,
    pub(super) server_control_client: bool,
    pub(super) unix_listener: bool,
    pub(super) unix_stream: bool,
    pub(super) channel_receiver: bool,
    pub(super) memory_queue: bool,
    pub(super) type_arguments: Vec<(String, Self)>,
    pub(super) shape: TypeShape,
    pub(super) alternatives: Vec<Self>,
}

impl TypeFact {
    pub(super) fn alternatives(facts: impl IntoIterator<Item = Self>) -> Self {
        let mut alternatives = Vec::new();
        for fact in facts {
            if fact.alternatives.is_empty() {
                if !alternatives.contains(&fact) {
                    alternatives.push(fact);
                }
            } else {
                for alternative in fact.alternatives {
                    if !alternatives.contains(&alternative) {
                        alternatives.push(alternative);
                    }
                }
            }
        }
        match alternatives.len() {
            0 => Self::default(),
            1 => alternatives.pop().expect("one type fact remains"),
            _ => Self {
                alternatives,
                ..Self::default()
            },
        }
    }

    pub(super) fn tuple(components: Vec<Self>) -> Self {
        Self {
            shape: TypeShape::Tuple(components),
            ..Self::default()
        }
    }

    pub(super) fn sequence(component: Self) -> Self {
        Self {
            shape: TypeShape::Sequence(Box::new(component)),
            ..Self::default()
        }
    }

    pub(super) fn variants(&self) -> std::slice::Iter<'_, Self> {
        if self.alternatives.is_empty() {
            std::slice::from_ref(self).iter()
        } else {
            self.alternatives.iter()
        }
    }

    pub(super) fn any_variant(&self, predicate: impl Fn(&Self) -> bool) -> bool {
        self.variants().any(predicate)
    }

    pub(super) fn all_variants(&self, predicate: impl Fn(&Self) -> bool) -> bool {
        self.variants().all(predicate)
    }

    pub(super) fn sole_canonical(&self) -> Option<&str> {
        let mut canonicals = self
            .variants()
            .filter_map(|variant| variant.canonical.as_deref());
        let first = canonicals.next()?;
        canonicals
            .all(|canonical| canonical == first)
            .then_some(first)
    }

    pub(super) fn mark_borrowed(mut self) -> Self {
        if self.alternatives.is_empty() {
            self.borrowed = true;
        } else {
            for alternative in &mut self.alternatives {
                alternative.borrowed = true;
            }
        }
        self
    }

    pub(super) fn tuple_component(&self, index: usize) -> Self {
        Self::alternatives(self.variants().filter_map(|variant| {
            let component = match &variant.shape {
                TypeShape::Tuple(components) => components.get(index).cloned(),
                _ => None,
            }?;
            Some(inherit_borrow(variant, component))
        }))
    }

    pub(super) fn tuple_component_from_end(&self, index: usize) -> Self {
        Self::alternatives(self.variants().filter_map(|variant| {
            match &variant.shape {
                TypeShape::Tuple(components) => components
                    .len()
                    .checked_sub(index + 1)
                    .and_then(|index| components.get(index))
                    .cloned()
                    .map(|component| inherit_borrow(variant, component)),
                _ => None,
            }
        }))
    }

    pub(super) fn sequence_component(&self) -> Self {
        Self::alternatives(self.variants().filter_map(|variant| {
            let component = match &variant.shape {
                TypeShape::Sequence(component) => (**component).clone(),
                _ => return None,
            };
            Some(inherit_borrow(variant, component))
        }))
    }

    pub(super) fn sequence_remainder(&self) -> Self {
        Self::alternatives(self.variants().filter_map(|variant| {
            let component = match &variant.shape {
                TypeShape::Sequence(component) => (**component).clone(),
                _ => return None,
            };
            Some(inherit_borrow(variant, Self::sequence(component)))
        }))
    }
}

fn inherit_borrow(owner: &TypeFact, component: TypeFact) -> TypeFact {
    if owner.borrowed {
        component.mark_borrowed()
    } else {
        component
    }
}
