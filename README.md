<p align="center">
	<img width="550" src="https://raw.githubusercontent.com/JSH32/tinybase/master/.github/banner.png"><br>
	<img src="https://img.shields.io/badge/contributions-welcome-orange.svg">
	<img src="https://img.shields.io/badge/Made%20with-%E2%9D%A4-ff69b4?logo=love">
</p>

# TinyBase

TinyBase is an in-memory database built with Rust, based on the [sled](https://github.com/spacejam/sled) embedded key-value store. It supports indexing and constraints, allowing you to create efficient queries and ensure data consistency.

## Features
- In-memory storage for fast access.
- Built on top of sled for a reliable key-value store.
- Indexing support for efficient querying.
- Constraints to ensure data consistency.

## Installation & Setup

To use TinyBase in your Rust project, add the following line to your Cargo.toml file's `[dependencies]` section:

```toml
tinybase = "0.1.1"
```

## Usage Example

Here's a simple example demonstrating how to use TinyBase with a `Person` struct:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Person {
    name: String,
    last_name: String,
}

fn main() {
    let db = TinyBase::new(Some("./people"), true);
    let person_table: Table<Person> = db.open_table("people").unwrap();

    let name_idx = person_table
        .create_index("name", |record| record.name.to_owned())
        .unwrap();

    let lastname_idx = person_table
        .create_index("last_name", |record| record.last_name.to_owned())
        .unwrap();

    person_table
        .constraint(Constraint::unique(&name_idx))
        .unwrap();

    person_table
        .constraint(Constraint::check(|person| !person.name.contains(".")))
        .unwrap();

    init_example_data(&person_table);

    println!(
        "{:#?}",
        QueryBuilder::new(&person_table)
            .by(&name_idx, "John".to_string())
            .by(&lastname_idx, "Jones".to_string())
            .update(
                QueryOperator::Or,
                Person {
                    name: "Kevin".to_string(),
                    last_name: "Spacey".to_string()
                }
            )
            .unwrap()
    );
}

fn init_example_data(person_table: &Table<Person>) {
    person_table
        .insert(Person {
            name: "John".to_string(),
            last_name: "Smith".to_string(),
        })
        .unwrap();

    person_table
        .insert(Person {
            name: "Bill".to_string(),
            last_name: "Smith".to_string(),
        })
        .unwrap();

    person_table
        .insert(Person {
            name: "Coraline".to_string(),
            last_name: "Jones".to_string(),
        })
        .unwrap();
}
```

This example demonstrates how to create a new TinyBase instance, open a table (or create one if it doesn't exist), add indexes and constraints, and perform basic operations (insert/select).

You can view more examples in [examples](https://github.com/JSH32/tinybase/tree/master/examples)
