use super::task::Task;

use parking_lot::Mutex;
use rand::{Rng, thread_rng, distributions::Alphanumeric};

use rocket::local::Client;
use rocket::http::{Status, ContentType};

// We use a lock to synchronize between tests so DB operations don't collide.
// For now. In the future, we'll have a nice way to run each test in a DB
// transaction so we can regain concurrency.
static DB_LOCK: Mutex<()> = Mutex::new(());

macro_rules! run_test {
    (|$client:ident, $conn:ident| $block:expr) => ({
        let _lock = DB_LOCK.lock();

        rocket::async_test(async move {
            let rocket = super::rocket();
            let db = super::DbConn::get_one(&rocket);
            let $client = Client::new(rocket).expect("Rocket client");
            let $conn = db.expect("failed to get database connection for testing");
            Task::delete_all(&$conn).expect("failed to delete all tasks for testing");

            $block
        })
    })
}

#[test]
fn test_insertion_deletion() {
    run_test!(|client, conn| {
        // Get the tasks before making changes.
        let init_tasks = Task::all(&conn).unwrap();

        // Issue a request to insert a new task.
        client.post("/todo")
            .header(ContentType::Form)
            .body("description=My+first+task")
            .dispatch().await;

        // Ensure we have one more task in the database.
        let new_tasks = Task::all(&conn).unwrap();
        assert_eq!(new_tasks.len(), init_tasks.len() + 1);

        // Ensure the task is what we expect.
        assert_eq!(new_tasks[0].description, "My first task");
        assert_eq!(new_tasks[0].completed, false);

        // Issue a request to delete the task.
        let id = new_tasks[0].id.unwrap();
        client.delete(format!("/todo/{}", id)).dispatch().await;

        // Ensure it's gone.
        let final_tasks = Task::all(&conn).unwrap();
        assert_eq!(final_tasks.len(), init_tasks.len());
        if final_tasks.len() > 0 {
            assert_ne!(final_tasks[0].description, "My first task");
        }
    })
}

#[test]
fn test_toggle() {
    run_test!(|client, conn| {
        // Issue a request to insert a new task; ensure it's not yet completed.
        client.post("/todo")
            .header(ContentType::Form)
            .body("description=test_for_completion")
            .dispatch().await;

        let task = Task::all(&conn).unwrap()[0].clone();
        assert_eq!(task.completed, false);

        // Issue a request to toggle the task; ensure it is completed.
        client.put(format!("/todo/{}", task.id.unwrap())).dispatch().await;
        assert_eq!(Task::all(&conn).unwrap()[0].completed, true);

        // Issue a request to toggle the task; ensure it's not completed again.
        client.put(format!("/todo/{}", task.id.unwrap())).dispatch().await;
        assert_eq!(Task::all(&conn).unwrap()[0].completed, false);
    })
}

#[test]
fn test_many_insertions() {
    const ITER: usize = 100;

    run_test!(|client, conn| {
        // Get the number of tasks initially.
        let init_num = Task::all(&conn).unwrap().len();
        let mut descs = Vec::new();

        for i in 0..ITER {
            // Issue a request to insert a new task with a random description.
            let desc: String = thread_rng().sample_iter(&Alphanumeric).take(12).collect();
            client.post("/todo")
                .header(ContentType::Form)
                .body(format!("description={}", desc))
                .dispatch().await;

            // Record the description we choose for this iteration.
            descs.insert(0, desc);

            // Ensure the task was inserted properly and all other tasks remain.
            let tasks = Task::all(&conn).unwrap();
            assert_eq!(tasks.len(), init_num + i + 1);

            for j in 0..i {
                assert_eq!(descs[j], tasks[j].description);
            }
        }
    })
}

#[test]
fn test_bad_form_submissions() {
    run_test!(|client, _conn| {
        // Submit an empty form. We should get a 422 but no flash error.
        let res = client.post("/todo")
            .header(ContentType::Form)
            .dispatch().await;

        let mut cookies = res.headers().get("Set-Cookie");
        assert_eq!(res.status(), Status::UnprocessableEntity);
        assert!(!cookies.any(|value| value.contains("error")));

        // Submit a form with an empty description. We look for 'error' in the
        // cookies which corresponds to flash message being set as an error.
        let res = client.post("/todo")
            .header(ContentType::Form)
            .body("description=")
            .dispatch().await;

        let mut cookies = res.headers().get("Set-Cookie");
        assert!(cookies.any(|value| value.contains("error")));

        // Submit a form without a description. Expect a 422 but no flash error.
        let res = client.post("/todo")
            .header(ContentType::Form)
            .body("evil=smile")
            .dispatch().await;

        let mut cookies = res.headers().get("Set-Cookie");
        assert_eq!(res.status(), Status::UnprocessableEntity);
        assert!(!cookies.any(|value| value.contains("error")));
    })
}
