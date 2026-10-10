//! Design tables: one part in several sizes. A table has a column for each parameter it sets and
//! a named row for each size. The part is always at one row, the active one: that row's values
//! follow the part as it is edited, and making another row active gives the part that row's
//! values.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::expr;
use crate::params::ParamUnit;

/// Most columns and rows one table may hold (hostile-input cap).
pub const MAX_COLUMNS: usize = 100;
pub const MAX_ROWS: usize = 1000;

/// One size of the part: a value for each column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableRow {
    pub name: String,
    pub values: Vec<f64>,
}

/// The sizes of a part.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DesignTable {
    /// The row the part is at.
    pub active: String,
    /// The parameters the table sets, by name. Values are as the parameters show them:
    /// millimetres, degrees, or plain numbers.
    pub columns: Vec<String>,
    pub rows: Vec<TableRow>,
}

impl DesignTable {
    pub fn row(&self, name: &str) -> Option<&TableRow> {
        self.rows.iter().find(|r| r.name == name)
    }
    fn column(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
}

fn row_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 || n > 64 {
        return Err("a row's name must be 1 to 64 characters".into());
    }
    Ok(name.to_owned())
}

impl Document {
    pub fn table(&self) -> Option<&DesignTable> {
        self.table.as_ref()
    }

    /// The unit of the parameter `name` when a table can set it: it must exist and be a plain
    /// value, not the result of an equation (a table would overwrite the equation).
    fn table_column_unit(&self, name: &str) -> Result<ParamUnit, String> {
        if let Some(m) = self.params.model.iter().find(|m| m.name == name) {
            if self.driven_in(&m.target).is_some() {
                return Err(format!("{name} is a driven dimension: it follows its sketch, so a table cannot set it"));
            }
            if m.equation.is_some() {
                return Err(format!("{name} follows an equation: put the parameters it is worked out from in the table instead"));
            }
            return self.value_at(&m.target).map(|v| v.1).ok_or_else(|| format!("{name} has no value"));
        }
        if let Some(u) = self.params.user.iter().find(|u| u.name == name) {
            let mut names = Vec::new();
            expr::parse(&u.equation).map_err(|e| format!("{name}: {e}"))?.names(&mut names);
            if !names.is_empty() {
                return Err(format!("{name} is worked out from other parameters: put those in the table instead"));
            }
            return Ok(u.unit);
        }
        Err(format!("there is no parameter `{name}`"))
    }

    /// Gives the parameter `name` the value `v` (as shown), as a row of the table does.
    fn table_apply(&mut self, name: &str, v: f64) -> Result<(), String> {
        if !v.is_finite() {
            return Err(format!("{name}: not a number"));
        }
        if let Some(target) = self.params.model.iter().find(|m| m.name == name).map(|m| m.target.clone()) {
            return self.set_value_at(&target, v).map_err(|e| format!("{name} = {v}: {e}"));
        }
        match self.params.user.iter_mut().find(|u| u.name == name) {
            Some(u) => {
                let unit = u.unit.label();
                u.equation = if unit.is_empty() { format!("{v}") } else { format!("{v} {unit}") };
                Ok(())
            }
            None => Err(format!("there is no parameter `{name}`")),
        }
    }

    /// Starts a table: `columns` are the parameters it sets, and the part as it stands is its
    /// first row, `row`.
    pub fn table_create(&mut self, columns: &[String], row: &str) -> Result<(), String> {
        if self.table.is_some() {
            return Err("the part has a design table already".into());
        }
        if columns.is_empty() || columns.len() > MAX_COLUMNS {
            return Err(format!("a design table needs 1 to {MAX_COLUMNS} parameters"));
        }
        let unique: BTreeSet<&String> = columns.iter().collect();
        if unique.len() != columns.len() {
            return Err("a parameter is in the table only once".into());
        }
        for c in columns {
            self.table_column_unit(c)?;
        }
        let now = self.parameter_values();
        let values = columns.iter().map(|c| now.get(c).copied().ok_or_else(|| format!("{c} has no value"))).collect::<Result<_, _>>()?;
        let name = row_name(row)?;
        self.table = Some(DesignTable { active: name.clone(), columns: columns.to_vec(), rows: vec![TableRow { name, values }] });
        Ok(())
    }

    pub fn table_delete(&mut self) -> Result<(), String> {
        self.table.take().map(|_| ()).ok_or_else(|| "the part has no design table".into())
    }

    fn table_mut(&mut self) -> Result<&mut DesignTable, String> {
        self.table.as_mut().ok_or_else(|| "the part has no design table: make one first".into())
    }

    /// Adds a row. Its values start as the active row's (the part as it stands); `values` then
    /// sets some by column name.
    pub fn table_add_row(&mut self, name: &str, values: &[(String, f64)]) -> Result<(), String> {
        let name = row_name(name)?;
        let t = self.table_mut()?;
        if t.rows.len() >= MAX_ROWS {
            return Err("the table is full".into());
        }
        if t.row(&name).is_some() {
            return Err(format!("there is a row `{name}` already"));
        }
        let mut row = t.row(&t.active).cloned().ok_or("the table has no active row")?;
        row.name = name;
        for (column, v) in values {
            let i = t.column(column).ok_or_else(|| format!("the table has no column `{column}`"))?;
            if !v.is_finite() {
                return Err(format!("{column}: not a number"));
            }
            row.values[i] = *v;
        }
        t.rows.push(row);
        Ok(())
    }

    /// Removes a row. The active row cannot be removed: the part is at it.
    pub fn table_remove_row(&mut self, name: &str) -> Result<(), String> {
        let t = self.table_mut()?;
        if t.active == name {
            return Err(format!("the part is at row `{name}`: make another row active first"));
        }
        let i = t.rows.iter().position(|r| r.name == name).ok_or_else(|| format!("the table has no row `{name}`"))?;
        t.rows.remove(i);
        Ok(())
    }

    pub fn table_rename_row(&mut self, from: &str, to: &str) -> Result<(), String> {
        let to = row_name(to)?;
        let t = self.table_mut()?;
        if t.row(&to).is_some() {
            return Err(format!("there is a row `{to}` already"));
        }
        let row = t.rows.iter_mut().find(|r| r.name == from).ok_or_else(|| format!("the table has no row `{from}`"))?;
        row.name = to.clone();
        if t.active == from {
            t.active = to;
        }
        Ok(())
    }

    /// Sets one value of one row. In the active row that changes the part.
    pub fn table_set(&mut self, row: &str, column: &str, v: f64) -> Result<(), String> {
        if !v.is_finite() {
            return Err(format!("{column}: not a number"));
        }
        let t = self.table_mut()?;
        let i = t.column(column).ok_or_else(|| format!("the table has no column `{column}`"))?;
        let active = t.active == row;
        let r = t.rows.iter_mut().find(|r| r.name == row).ok_or_else(|| format!("the table has no row `{row}`"))?;
        r.values[i] = v;
        if active {
            self.table_apply(column, v)?;
        }
        Ok(())
    }

    /// Adds a parameter to the table: every row gets its present value.
    pub fn table_add_column(&mut self, name: &str) -> Result<(), String> {
        self.table_column_unit(name)?;
        let v = self.parameter_values().get(name).copied().ok_or_else(|| format!("{name} has no value"))?;
        let t = self.table_mut()?;
        if t.columns.len() >= MAX_COLUMNS {
            return Err("the table is full".into());
        }
        if t.column(name).is_some() {
            return Err(format!("{name} is in the table already"));
        }
        t.columns.push(name.to_owned());
        for r in &mut t.rows {
            r.values.push(v);
        }
        Ok(())
    }

    /// Takes a parameter out of the table: it keeps the value it has, in every row.
    pub fn table_remove_column(&mut self, name: &str) -> Result<(), String> {
        let t = self.table_mut()?;
        let i = t.column(name).ok_or_else(|| format!("the table has no column `{name}`"))?;
        if t.columns.len() == 1 {
            return Err("a table needs at least one parameter: delete the table instead".into());
        }
        t.columns.remove(i);
        for r in &mut t.rows {
            r.values.remove(i);
        }
        Ok(())
    }

    /// Makes `row` the active one: the part takes its values.
    pub fn table_activate(&mut self, row: &str) -> Result<(), String> {
        let t = self.table_mut()?;
        let r = t.row(row).cloned().ok_or_else(|| format!("the table has no row `{row}`"))?;
        let columns = t.columns.clone();
        t.active = r.name.clone();
        for (c, v) in columns.iter().zip(&r.values) {
            self.table_apply(c, *v)?;
        }
        Ok(())
    }

    /// Keeps the table true to the part after an edit: a parameter that was renamed keeps its
    /// column, one that is gone or now follows an equation leaves the table, and the active row
    /// takes the part's present values. Called with every change (see `sync_parameters`), and when a
    /// file is read: should a file edited by hand disagree with itself, the part's own values stand.
    pub fn table_follow(&mut self) {
        let Some(mut t) = self.table.take() else { return };
        let keep: Vec<bool> = t.columns.iter().map(|c| self.table_column_unit(c).is_ok()).collect();
        if keep.iter().any(|k| !k) {
            let mut i = 0;
            t.columns.retain(|_| {
                i += 1;
                keep[i - 1]
            });
            for r in &mut t.rows {
                let mut i = 0;
                r.values.retain(|_| {
                    i += 1;
                    keep[i - 1]
                });
            }
        }
        if t.columns.is_empty() {
            // Nothing left for it to set.
            return;
        }
        let now = self.parameter_values();
        let active = t.active.clone();
        let columns = t.columns.clone();
        if let Some(r) = t.rows.iter_mut().find(|r| r.name == active) {
            for (i, c) in columns.iter().enumerate() {
                if let Some(v) = now.get(c) {
                    r.values[i] = *v;
                }
            }
        }
        self.table = Some(t);
    }

    /// A renamed parameter keeps its column.
    pub(crate) fn table_renamed(&mut self, from: &str, to: &str) {
        if let Some(t) = &mut self.table {
            for c in &mut t.columns {
                if c == from {
                    *c = to.to_owned();
                }
            }
        }
    }

    /// Checks a table read from a file.
    pub(crate) fn table_check(&self) -> Result<(), String> {
        let Some(t) = &self.table else { return Ok(()) };
        if t.columns.is_empty() || t.columns.len() > MAX_COLUMNS || t.rows.is_empty() || t.rows.len() > MAX_ROWS {
            return Err(format!("the design table must have 1 to {MAX_COLUMNS} parameters and 1 to {MAX_ROWS} rows"));
        }
        let columns: BTreeSet<&String> = t.columns.iter().collect();
        if columns.len() != t.columns.len() {
            return Err("the design table names a parameter twice".into());
        }
        for c in &t.columns {
            if !self.params.model.iter().any(|m| &m.name == c) && !self.params.user.iter().any(|u| &u.name == c) {
                return Err(format!("the design table sets `{c}`, which is not a parameter of the part"));
            }
        }
        let mut rows = BTreeSet::new();
        for r in &t.rows {
            row_name(&r.name).map_err(|e| format!("design table: {e}"))?;
            if !rows.insert(&r.name) {
                return Err(format!("the design table has two rows named `{}`", r.name));
            }
            if r.values.len() != t.columns.len() || r.values.iter().any(|v| !v.is_finite()) {
                return Err(format!("row `{}` of the design table must have one number for each of its {} parameters", r.name, t.columns.len()));
            }
        }
        if t.row(&t.active).is_none() {
            return Err(format!("the design table's active row `{}` is not one of its rows", t.active));
        }
        Ok(())
    }
}
