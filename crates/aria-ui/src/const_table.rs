//! Compile-time concatenation of row groups.
//!
//! A shipped table, such as the key bindings or the palette commands, is
//! written as one slice per file and published as one array. [`total`] sums
//! the group lengths and [`joined`] copies the rows, both at compile time, so
//! no row count is written by hand and a row added to any group is in the table.

/// How many rows `groups` hold between them.
pub(crate) const fn total<T>(groups: &[&[T]]) -> usize {
    let mut total = 0;
    let mut group = 0;
    while group < groups.len() {
        total += groups[group].len();
        group += 1;
    }
    total
}

/// The rows of `groups` flattened into one array, in group order.
///
/// `N` must be [`total`] of the same groups. Evaluated in a constant, a wrong
/// `N` or an empty first group fails the build instead of padding the table
/// with copies of its first row.
pub(crate) const fn joined<const N: usize, T: Copy>(groups: &[&[T]]) -> [T; N] {
    let mut out = [groups[0][0]; N];
    let mut at = 0;
    let mut group = 0;
    while group < groups.len() {
        let rows = groups[group];
        let mut row = 0;
        while row < rows.len() {
            out[at] = rows[row];
            at += 1;
            row += 1;
        }
        group += 1;
    }
    assert!(at == N, "N is not the row count of the groups");
    out
}

#[cfg(test)]
mod tests {
    use super::{joined, total};

    const GROUPS: &[&[u8]] = &[&[1, 2], &[], &[3], &[4, 5]];

    #[test]
    fn the_groups_join_in_order_with_every_row_once() {
        const TOTAL: usize = total(GROUPS);
        assert_eq!(TOTAL, 5);
        assert_eq!(joined::<TOTAL, u8>(GROUPS), [1, 2, 3, 4, 5]);
    }

    #[test]
    #[should_panic(expected = "N is not the row count of the groups")]
    fn a_count_above_the_rows_is_refused() {
        let _ = joined::<6, u8>(GROUPS);
    }
}
