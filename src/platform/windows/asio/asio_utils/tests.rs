use super::{deinterleave, deinterleave_index, interleave};

#[test]
fn interleave_two() {
    let a = vec![vec![1, 1, 1, 1], vec![2, 2, 2, 2]];
    let goal = vec![1, 2, 1, 2, 1, 2, 1, 2];
    let mut result = vec![0; 8];

    interleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn interleave_three() {
    let a = vec![vec![1, 1, 1, 1], vec![2, 2, 2, 2], vec![3, 3, 3, 3]];
    let goal = vec![1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3];
    let mut result = vec![0; 12];

    interleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn interleave_none() {
    let a = vec![Vec::<i32>::new()];
    let goal = Vec::<i32>::new();
    let mut result = Vec::<i32>::new();

    interleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn interleave_two_diff() {
    let a = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]];
    let goal = vec![1, 5, 2, 6, 3, 7, 4, 8];
    let mut result = vec![0; 8];

    interleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn deinterleave_two() {
    let goal = vec![vec![1, 1, 1, 1], vec![2, 2, 2, 2]];
    let a = vec![1, 2, 1, 2, 1, 2, 1, 2];
    let mut result = vec![vec![0; 4]; 2];

    deinterleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn deinterleave_three() {
    let goal = vec![vec![1, 1, 1, 1], vec![2, 2, 2, 2], vec![3, 3, 3, 3]];
    let a = vec![1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3];
    let mut result = vec![vec![0; 4]; 3];

    deinterleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn deinterleave_none() {
    let goal = vec![Vec::<i32>::new()];
    let a = Vec::<i32>::new();
    let mut result = vec![Vec::<i32>::new()];

    deinterleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}

#[test]
fn deinterleave_two_diff() {
    let goal = vec![vec![1, 2, 3, 4], vec![5, 6, 7, 8]];
    let a = vec![1, 5, 2, 6, 3, 7, 4, 8];
    let mut result = vec![vec![0; 4]; 2];

    deinterleave(&a[..], &mut result[..]);

    assert_eq!(goal, result);
}
