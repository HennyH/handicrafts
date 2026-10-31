//! An order's status and the rules for changing it. The app is a record of
//! facts: the admin does the work outside it and writes down what happened.
//! Only a verified Monokulo webhook moves an order on by itself.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    New,
    Invoiced,
    Paid,
    Dispatched,
    Completed,
    Rejected,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::New,
        Status::Invoiced,
        Status::Paid,
        Status::Dispatched,
        Status::Completed,
        Status::Rejected,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Invoiced => "invoiced",
            Status::Paid => "paid",
            Status::Dispatched => "dispatched",
            Status::Completed => "completed",
            Status::Rejected => "rejected",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Status::New => "New",
            Status::Invoiced => "Invoiced",
            Status::Paid => "Paid",
            Status::Dispatched => "Dispatched",
            Status::Completed => "Completed",
            Status::Rejected => "Rejected",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|st| st.as_str() == s)
    }

    /// Invoiced, Paid, Dispatched and Completed can only be chosen with
    /// Accepted ticked.
    pub fn needs_accepted(self) -> bool {
        matches!(
            self,
            Status::Invoiced | Status::Paid | Status::Dispatched | Status::Completed
        )
    }

    fn rank(self) -> u8 {
        match self {
            Status::New => 0,
            Status::Invoiced => 1,
            Status::Paid => 2,
            Status::Dispatched => 3,
            Status::Completed => 4,
            Status::Rejected => 9,
        }
    }

    /// Where the admin may move an order from here. Forward along the
    /// accepted path (skipping is fine, say a cash sale at a market), Rejected
    /// from anything not yet posted, and Rejected back to New to undo a
    /// mistake. Never backwards otherwise.
    pub fn next(self) -> Vec<Status> {
        match self {
            Status::Rejected => vec![Status::New],
            Status::Completed => vec![],
            from => {
                let mut v: Vec<Status> = Status::ALL
                    .into_iter()
                    .filter(|s| *s != Status::Rejected && s.rank() > from.rank())
                    .collect();
                if matches!(from, Status::New | Status::Invoiced | Status::Paid) {
                    v.push(Status::Rejected);
                }
                v
            }
        }
    }
}

/// Check an admin's status change. `Ok` gives the `accepted` flag to store.
pub fn admin_change(
    from: Status,
    was_accepted: bool,
    to: Status,
    accepted: bool,
) -> Result<bool, String> {
    if to == from && accepted == was_accepted {
        return Err("Nothing changed.".into());
    }
    if to != from && !from.next().contains(&to) {
        return Err(format!(
            "An order can't go from {} to {}.",
            from.label(),
            to.label()
        ));
    }
    if to.needs_accepted() && !accepted {
        return Err(format!("Tick Accepted before setting {}.", to.label()));
    }
    // Rejecting clears Accepted.
    Ok(if to == Status::Rejected {
        false
    } else {
        accepted
    })
}

/// Whether a verified Monero payment moves this order to Paid.
pub fn webhook_pays(from: Status) -> bool {
    matches!(from, Status::New | Status::Invoiced)
}

#[cfg(test)]
mod tests {
    use super::Status::*;
    use super::*;

    #[test]
    fn transitions() {
        assert_eq!(
            New.next(),
            vec![Invoiced, Paid, Dispatched, Completed, Rejected]
        );
        assert_eq!(Invoiced.next(), vec![Paid, Dispatched, Completed, Rejected]);
        assert_eq!(Paid.next(), vec![Dispatched, Completed, Rejected]);
        assert_eq!(Dispatched.next(), vec![Completed]);
        assert!(Completed.next().is_empty());
        assert_eq!(Rejected.next(), vec![New]);
    }

    #[test]
    fn accepted_gates_the_accepted_path() {
        assert!(admin_change(New, false, Invoiced, false).is_err());
        assert_eq!(admin_change(New, false, Invoiced, true), Ok(true));
        assert_eq!(admin_change(New, false, Rejected, false), Ok(false));
        assert_eq!(
            admin_change(Invoiced, true, Rejected, true),
            Ok(false),
            "rejecting clears Accepted"
        );
        assert!(
            admin_change(Paid, true, Invoiced, true).is_err(),
            "no going back"
        );
        assert_eq!(admin_change(Rejected, false, New, false), Ok(false));
        assert_eq!(
            admin_change(New, false, New, true),
            Ok(true),
            "just ticking Accepted"
        );
        assert!(
            admin_change(Invoiced, true, Invoiced, false).is_err(),
            "can't untick while invoiced"
        );
        assert!(admin_change(New, false, New, false).is_err());
    }

    #[test]
    fn webhook() {
        assert!(webhook_pays(New));
        assert!(webhook_pays(Invoiced));
        assert!(!webhook_pays(Paid));
        assert!(!webhook_pays(Rejected));
    }
}
