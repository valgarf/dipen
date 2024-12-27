// note: just for fun and because i was interested why the push_str version was faster than concat...
// expands upon https://users.rust-lang.org/t/fast-string-concatenation/4425/6

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

fn format_url_macro(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_macro", |b| b.iter(|| format!("/{}/_alias/{}", index, name)));
}

fn format_url_concat(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_concat", |b| {
        b.iter(|| {
            let mut url = "/".to_string();
            url = url + &index[..] + "/_alias/" + &name[..];
            url
        })
    });
}

fn format_url_push(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_push", |b| {
        b.iter(|| {
            let mut url = String::with_capacity(
                1 + "/_alias/".len() + (&index[..]).len() + (&name[..]).len(),
            );
            url.push_str("/");
            url.push_str(&index[..]);
            url.push_str("/_alias/");
            url.push_str(&name[..]);
            url
        })
    });
}

fn format_url_write(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_write", |b| {
        b.iter(|| {
            use std::fmt::Write;
            let mut url = String::with_capacity(1 + "/_alias/".len() + index.len() + name.len());
            write!(url, "/{}/_alias/{}", index, name).unwrap();
            url
        })
    });
}

fn format_url_vec_concat(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_vec_concat", |b| {
        b.iter(|| {
            let url = vec!["/", &index[..], "/_alias/", &name[..]].concat();
            url
        })
    });
}

fn format_url_array_concat(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_array_concat", |b| {
        b.iter(|| {
            let url = ["/", &index, "/_alias/", &name].concat();
            url
        })
    });
}

fn format_url_join(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_join", |b| {
        b.iter(|| {
            let url = ["/", &index[..], "/_alias/", &name[..]].join("");
            url
        })
    });
}

fn opt_concat(args: &[&str]) -> String {
    let size = args.iter().map(|a| a.len()).sum::<usize>() + 1;
    let mut result = String::with_capacity(size);
    for arg in args {
        result.push_str(arg);
    }
    result
}

fn format_url_opt(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_opt", |b| {
        b.iter(|| {
            let url = opt_concat(&["/", &index[..], "/_alias/", &name[..]]);
            url
        })
    });
}

pub trait InlineConcat {
    fn concat_inline(&self) -> String;
}

impl InlineConcat for [&str] {
    #[inline(always)]
    fn concat_inline(&self) -> String {
        let size = self.iter().map(|a| a.len()).sum::<usize>() + 1;
        let mut result = String::with_capacity(size);
        for &arg in self {
            result.push_str(arg);
        }
        result
    }
}

fn format_url_inline(c: &mut Criterion) {
    let index = "test_idx".to_string();
    let name = "test_alias".to_string();

    c.bench_function("format_url_opt_inline", |b| {
        b.iter(|| {
            let url = ["/", &index[..], "/_alias/", &name[..]].concat_inline();
            url
        })
    });
}

criterion_group!(
    string_benches,
    format_url_macro,
    format_url_concat,
    format_url_push,
    format_url_write,
    format_url_vec_concat,
    format_url_array_concat,
    format_url_join,
    format_url_opt,
    format_url_inline
);
criterion_main!(string_benches);
