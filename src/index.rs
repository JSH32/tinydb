use std::any::Any;
use std::ops::Deref;
use std::sync::Arc;
use std::vec;

use bincode::{deserialize, serialize};
use sled::{Db, IVec, Tree};
use uuid::Uuid;

use crate::record::Record;
use crate::result::DbResult;
use crate::subscriber::{self, Subscriber};
use crate::table::TableType;

pub trait IndexType: AsRef<[u8]> {}
impl<T: AsRef<[u8]>> IndexType for T {}

/// An index of a Table.
///
/// # Type Parameters
///
/// * `T` - The type of the value to be stored in the table.
/// * `I` - The type of the index key.
pub struct Index<T: TableType, I: IndexType>(pub(crate) Arc<IndexInner<T, I>>);

impl<T: TableType, I: IndexType> Clone for Index<T, I> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: TableType, I: IndexType> Deref for Index<T, I> {
    type Target = Arc<IndexInner<T, I>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct IndexInner<T: TableType, I: IndexType> {
    table_data: Tree,
    /// Function which will be used to compute the key per insert.
    key_func: Box<dyn Fn(&T) -> I + Send + Sync>,
    /// Built index, each key can have multiple matching records.
    indexed_data: Tree,
    /// Reference to uncommitted operation log.
    subscriber: Subscriber<T>,
}

impl<T: TableType, I: IndexType> IndexInner<T, I> {
    pub(crate) fn new(
        idx_name: &str,
        engine: &Db,
        table_data: &Tree,
        key_func: impl Fn(&T) -> I + Send + Sync + 'static,
        subscriber: Subscriber<T>,
    ) -> DbResult<Self> {
        let need_sync = !engine.tree_names().contains(&IVec::from(idx_name));

        let new_index = Self {
            table_data: table_data.clone(),
            key_func: Box::new(key_func),
            indexed_data: engine.open_tree(idx_name)?,
            subscriber,
        };

        // Index is new, sync data
        if need_sync {
            new_index.sync()?;
        }

        Ok(new_index)
    }

    /// Resync index to be up to date with table.
    pub fn sync(&self) -> DbResult<()> {
        self.indexed_data.clear()?;
        for key in self.table_data.iter().keys() {
            // This should always succeed
            if let Some(data) = self.table_data.get(&key.clone()?)? {
                self.insert(&Record {
                    id: deserialize(&key?)?,
                    data: deserialize(&data)?,
                })?;
            }
        }

        Ok(())
    }

    fn commit_log(&self) -> DbResult<()> {
        // Commit log of events on the main table.
        while let Ok(event) = self.subscriber.rx.try_recv() {
            match event {
                subscriber::Event::Remove(record) => self.delete(&record)?,
                subscriber::Event::Insert(record) => self.insert(&record)?,
                subscriber::Event::Update {
                    id,
                    old_data,
                    new_data,
                } => {
                    self.delete(&Record { id, data: old_data })?;
                    self.insert(&Record { id, data: new_data })?;
                }
            }
        }

        Ok(())
    }

    /// Insert a record into the index.
    fn insert(&self, record: &Record<T>) -> DbResult<()> {
        let key = (self.key_func)(&record.data);

        if let Some(data) = self.indexed_data.get(&key)? {
            let mut vec: Vec<Uuid> = deserialize(&data)?;
            vec.push(record.id);
            self.indexed_data.insert(key, serialize(&vec)?)?;
        } else {
            self.indexed_data
                .insert(key, serialize(&vec![record.id])?)?;
        }

        Ok(())
    }

    /// Delete record from index.
    fn delete(&self, record: &Record<T>) -> DbResult<()> {
        let key = (self.key_func)(&record.data);

        if let Some(data) = self.indexed_data.get(&key)? {
            let mut index_values: Vec<Uuid> = deserialize(&data)?;

            // We can remove the entire node here since its one element.
            if index_values.len() < 2 {
                self.indexed_data.remove(&key)?;
            } else {
                // Remove the single ID from here.
                if let Some(pos) = index_values.iter().position(|id| *id == record.id) {
                    index_values.remove(pos);
                    // Replace the row with one that doesn't have the element.
                    self.indexed_data.insert(&key, serialize(&index_values)?)?;
                }
            }
        }

        Ok(())
    }

    /// Query by index key.
    ///
    /// This method searches for multiple [`Record`]'s that match the index key provided.
    ///
    /// # Arguments
    ///
    /// * `query` - A reference to the query key.
    ///
    /// # Errors
    ///
    /// Returns an error if the query could not be performed.
    ///
    /// # Example
    ///
    /// ```
    /// use tinydb::{TinyDb, Table, Index};
    ///
    /// let db = TinyDb::new(Some("path/to/db"), false);
    /// let mut table: Table<String> = db.open_table("my_table").unwrap();
    /// let mut index: Index<String, Vec<u8>> = table.create_index("my_index", |value| value.as_bytes().to_vec()).unwrap();
    /// let results: Vec<Record<String>> = index.query(&"my_value".as_bytes().to_vec()).unwrap();
    /// ```
    pub fn select(&self, query: &I) -> DbResult<Vec<Record<T>>> {
        self.commit_log()?;

        Ok(if let Ok(Some(bytes)) = self.indexed_data.get(query) {
            let uuids: Vec<Uuid> = deserialize(&bytes)?;

            let mut results = vec![];
            for uuid in uuids {
                let encoded_data = self.table_data.get(serialize(&uuid)?)?;
                if let Some(encoded_data) = encoded_data {
                    results.push(Record {
                        id: uuid,
                        data: deserialize::<T>(&encoded_data)?,
                    })
                }
            }

            results
        } else {
            Vec::new()
        })
    }

    /// Check if a record matches the built index key.
    pub fn exists(&self, record: &Record<T>) -> DbResult<bool> {
        let key = (self.key_func)(&record.data);
        Ok(!self.select(&key)?.is_empty())
    }

    pub fn index_name(&self) -> String {
        std::str::from_utf8(&self.indexed_data.name())
            .unwrap()
            .to_string()
    }
}

/// Type which [`Index`] can be casted to which doesn't require the `I` type parameter.
pub trait AnyIndex<T: TableType> {
    fn record_exists(&self, record: &Record<T>) -> DbResult<bool>;
    fn search(&self, value: Box<dyn Any>) -> DbResult<Vec<Record<T>>>;
    fn idx_name(&self) -> String;
}

impl<T, I> AnyIndex<T> for Index<T, I>
where
    T: TableType,
    I: IndexType + 'static,
{
    fn search(&self, value: Box<dyn Any>) -> DbResult<Vec<Record<T>>> {
        let i = *value.downcast::<I>().unwrap();
        self.select(&i)
    }

    fn idx_name(&self) -> String {
        self.index_name()
    }

    fn record_exists(&self, record: &Record<T>) -> DbResult<bool> {
        self.exists(record)
    }
}
