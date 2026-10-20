//! The ego view's layout model (TUI_SPEC §4.3): which groups go on which
//! side, in what order, and the cursor-navigable items of each column. Pure
//! functions of the `EgoView` the store already paginated.

use spektr_engine::{Direction, EgoGroup, EgoRow, EgoView};
use spektr_model::{CorePredicate, Predicate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Col {
    Left,
    Card,
    Right,
}

/// One cursor stop in a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// `group` indexes `EgoView::groups`.
    Row { group: usize, row: usize },
    /// The `··· N more ] p/q` line.
    More { group: usize },
}

/// Group priority: identity predicates first (TUI_SPEC §4.3).
pub fn predicate_rank(p: &Predicate) -> usize {
    use CorePredicate as P;
    const ORDER: [CorePredicate; 26] = [
        P::UsesHandle,
        P::HasAccount,
        P::VariantOf,
        P::SharesArtifact,
        P::HasEmail,
        P::HasPhone,
        P::RelatedTo,
        P::WorksAt,
        P::StudiedAt,
        P::LocatedIn,
        P::Posted,
        P::TakenAt,
        P::Depicts,
        P::LinksTo,
        P::HostedAt,
        P::AtDomain,
        P::OnHost,
        P::SubdomainOf,
        P::ResolvesTo,
        P::ReverseResolvesTo,
        P::MailHandledBy,
        P::RegisteredBy,
        P::IssuedFor,
        P::HostedInAsn,
        P::OperatedBy,
        P::AppearsInBreach,
    ];
    p.core()
        .and_then(|c| ORDER.iter().position(|o| *o == c))
        .unwrap_or(ORDER.len())
}

/// Inbound and symmetric groups on the left, outbound on the right, each
/// ordered by priority then count.
pub fn sides(ego: &EgoView) -> (Vec<usize>, Vec<usize>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (i, g) in ego.groups.iter().enumerate() {
        match g.key.direction {
            Direction::Out => right.push(i),
            Direction::In | Direction::Sym => left.push(i),
        }
    }
    let key = |i: &usize| {
        let g = &ego.groups[*i];
        (
            predicate_rank(&g.key.predicate),
            usize::MAX - g.total,
            g.key.clone(),
        )
    };
    left.sort_by_key(key);
    right.sort_by_key(key);
    (left, right)
}

/// The cursor stops of a column's groups.
pub fn items(ego: &EgoView, groups: &[usize]) -> Vec<Item> {
    let mut out = Vec::new();
    for &g in groups {
        let group = &ego.groups[g];
        out.extend((0..group.rows.len()).map(|row| Item::Row { group: g, row }));
        if group.pages > 1 {
            out.push(Item::More { group: g });
        }
    }
    out
}

pub fn column_items(ego: &EgoView, col: Col) -> Vec<Item> {
    let (l, r) = sides(ego);
    match col {
        Col::Left => items(ego, &l),
        Col::Right => items(ego, &r),
        Col::Card => Vec::new(),
    }
}

pub fn row(ego: &EgoView, item: Item) -> Option<(&EgoGroup, &EgoRow)> {
    match item {
        Item::Row { group, row } => {
            let g = ego.groups.get(group)?;
            Some((g, g.rows.get(row)?))
        }
        Item::More { .. } => None,
    }
}

/// Rows per group page (TUI_SPEC §4.3).
pub const PAGE: usize = 5;

/// Rows after the shown page: `··· N more`.
pub fn remaining(g: &EgoGroup) -> usize {
    g.total.saturating_sub(g.page * PAGE + g.rows.len())
}

/// Short labels for the focus card's count lines.
pub fn count_label(p: &Predicate) -> Option<&'static str> {
    use CorePredicate as P;
    Some(match p.core()? {
        P::HasAccount => "accts",
        P::VariantOf => "vars",
        P::HasEmail => "emails",
        P::HasPhone => "phones",
        P::SharesArtifact => "corr",
        P::SubdomainOf => "subs",
        P::ResolvesTo => "ips",
        P::IssuedFor => "certs",
        P::MailHandledBy => "mx",
        P::LinksTo => "links",
        P::AtDomain => "domain",
        P::OnHost => "host",
        P::RegisteredBy => "reg",
        P::HostedInAsn => "asn",
        P::RelatedTo => "rel",
        P::WorksAt => "work",
        P::LocatedIn => "place",
        _ => return None,
    })
}

/// What a correlated group's footer says the shared artifact is.
pub fn artifact_note(a: spektr_model::ArtifactKind) -> &'static str {
    use spektr_model::ArtifactKind as K;
    match a {
        K::AvatarPhash => "avatar hash match",
        K::AnalyticsId => "same analytics id",
        K::FaviconMmh3 => "favicon hash match",
        K::RegistrantEmail => "same registrant email",
        K::CertSan => "shared certificate",
        K::Ip => "same address",
    }
}
