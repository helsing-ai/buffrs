use super::*;
use proptest::prelude::*;
use sqlx::testing::{TestArgs, TestFn};

#[sqlx::test]
async fn can_migrate(pool: Pool) {}

#[sqlx::test]
async fn can_create_user(pool: Pool) {
    let mut connection = pool.acquire().await.unwrap();
    let user = connection.user_create("abc").await.unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10))]

    #[test]
    fn can_proptest(name in "[a-z]{16}") {
        println!("running {name}");
        let mut args = TestArgs::new("can_proptest");
        args.fixtures(&[]);
        async fn inner(pool: Pool) {
        }
        let f: fn(_) -> _ = inner;
        TestFn::run_test(f, args);
    }

    #[test]
    fn can_2proptest(name in "[a-z]{16}") {
        println!("running {name}");
        let mut args = TestArgs::new("can_proptest");
        args.fixtures(&[]);
        async fn inner(pool: Pool) {
        }
        let f: fn(_) -> _ = inner;
        TestFn::run_test(f, args);
    }

    #[test]
    fn can_3proptest(name in "[a-z]{16}") {
        println!("running {name}");
        let mut args = TestArgs::new("can_proptest");
        args.fixtures(&[]);
        async fn inner(pool: Pool) {
        }
        let f: fn(_) -> _ = inner;
        TestFn::run_test(f, args);
    }
}
