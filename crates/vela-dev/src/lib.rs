pub mod record;

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use record::DevelopmentRecord;
use serde::Serialize;
use vela_extensions::{ExtensionKind, ExtensionRegistry, activate_tool_selection};
use vela_kernel::scheduler::{
    FixedIntervalRecurrence, OccurrenceCount, RecurrenceId, RecurrenceStore, ScheduleCancellation,
    ScheduleHistoryEvent, ScheduleId, ScheduleInstant, ScheduleInterval, ScheduleRelease,
    ScheduleStatus, ScheduleStore, ScheduledTask,
};
use vela_kernel::task::{TaskGoal, TaskId};
use vela_kernel::tool::{
    PermissionDecision, ToolAuthorizer, ToolEffect, ToolId, ToolRegistry, ToolRequest,
};

/// Project Vela's developer-facing command line.
#[derive(Debug, Parser)]
#[command(name = "vela-dev", about = "Developer tooling for Project Vela")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Top-level developer workflows.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Work with Vela development records.
    Record {
        #[command(subcommand)]
        command: Option<RecordCommand>,
    },
    /// Inspect a directory of Vela development records.
    Corpus {
        #[command(subcommand)]
        command: Option<CorpusCommand>,
    },
    /// Work with validated Vela extension packages.
    Extension {
        #[command(subcommand)]
        command: Option<ExtensionCommand>,
    },
    /// Work with durable Vela schedule intent and evidence.
    Schedule {
        #[command(subcommand)]
        command: Option<ScheduleCommand>,
    },
    /// Work with durable finite recurrence definitions.
    Recurrence {
        #[command(subcommand)]
        command: Option<RecurrenceCommand>,
    },
}

/// Finite recurrence workflows.
#[derive(Debug, Subcommand)]
pub enum RecurrenceCommand {
    /// Persist one inert finite fixed-interval recurrence definition.
    Create {
        database: PathBuf,
        id: String,
        goal: String,
        anchor_unix_millis: u64,
        interval_millis: u64,
        occurrence_count: u64,
    },
    /// Print every finite recurrence through a read-only storage boundary.
    Inspect { database: PathBuf },
}

/// Durable schedule workflows.
#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    /// Persist one inert durable schedule intent.
    Create {
        database: PathBuf,
        id: String,
        goal: String,
        due_at_unix_millis: u64,
    },
    /// Cancel one exact pending schedule revision.
    Cancel {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        reason: String,
    },
    /// Claim one exact due schedule revision against a caller-owned cutoff.
    Claim {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        cutoff_unix_millis: u64,
    },
    /// Claim the earliest due schedule against a caller-owned cutoff.
    ClaimNext {
        database: PathBuf,
        cutoff_unix_millis: u64,
    },
    /// Release one exact claimed schedule revision with recovery evidence.
    Release {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        reason: String,
    },
    /// Materialize one exact claimed schedule revision as an inert active task.
    Materialize {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        task_id: String,
    },
    /// Atomically materialize the earliest due schedule as an inert active task.
    MaterializeNext {
        database: PathBuf,
        cutoff_unix_millis: u64,
        task_id: String,
    },
    /// Print every durable schedule through a read-only storage boundary.
    Inspect { database: PathBuf },
    /// Print one exact durable schedule through a read-only storage boundary.
    Get { database: PathBuf, id: String },
    /// Print durable schedules with one exact lifecycle status.
    Status { database: PathBuf, status: String },
    /// Print pending schedules due by one explicit Unix-millisecond cutoff.
    Due {
        database: PathBuf,
        cutoff_unix_millis: u64,
    },
    /// Print one exact schedule's validated lifecycle history.
    History { database: PathBuf, id: String },
    /// Resolve one materialized schedule from an exact task identity.
    Task { database: PathBuf, task_id: String },
}

/// Extension-package workflows.
#[derive(Debug, Subcommand)]
pub enum ExtensionCommand {
    /// Discover and print one validated extension root.
    Inspect { root: PathBuf },
    /// Invoke one exact validated WebAssembly tool with a JSON value.
    Invoke {
        root: PathBuf,
        id: String,
        input_json: String,
    },
}

/// Corpus workflows.
#[derive(Debug, Subcommand)]
pub enum CorpusCommand {
    /// Recursively validate JSON records and summarize the corpus.
    Inspect { path: PathBuf },
}

/// Development-record workflows.
#[derive(Debug, Subcommand)]
pub enum RecordCommand {
    /// Validate one schema-versioned JSON development record.
    Validate { path: PathBuf },
}

impl Cli {
    #[must_use]
    pub fn run(self) -> ExitCode {
        match self.command {
            Some(Command::Record {
                command: Some(RecordCommand::Validate { path }),
            }) => validate_record(&path),
            Some(Command::Corpus {
                command: Some(CorpusCommand::Inspect { path }),
            }) => inspect_corpus(&path),
            Some(Command::Extension {
                command: Some(ExtensionCommand::Inspect { root }),
            }) => inspect_extensions(&root),
            Some(Command::Extension {
                command:
                    Some(ExtensionCommand::Invoke {
                        root,
                        id,
                        input_json,
                    }),
            }) => invoke_extension(&root, &id, &input_json),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Create {
                        database,
                        id,
                        goal,
                        due_at_unix_millis,
                    }),
            }) => create_schedule(&database, &id, &goal, due_at_unix_millis),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Cancel {
                        database,
                        id,
                        expected_revision,
                        reason,
                    }),
            }) => cancel_schedule(&database, &id, expected_revision, &reason),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Claim {
                        database,
                        id,
                        expected_revision,
                        cutoff_unix_millis,
                    }),
            }) => claim_schedule(&database, &id, expected_revision, cutoff_unix_millis),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::ClaimNext {
                        database,
                        cutoff_unix_millis,
                    }),
            }) => claim_next_schedule(&database, cutoff_unix_millis),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Release {
                        database,
                        id,
                        expected_revision,
                        reason,
                    }),
            }) => release_schedule(&database, &id, expected_revision, &reason),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Materialize {
                        database,
                        id,
                        expected_revision,
                        task_id,
                    }),
            }) => materialize_schedule(&database, &id, expected_revision, &task_id),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::MaterializeNext {
                        database,
                        cutoff_unix_millis,
                        task_id,
                    }),
            }) => materialize_next_schedule(&database, cutoff_unix_millis, &task_id),
            Some(Command::Schedule {
                command: Some(ScheduleCommand::Inspect { database }),
            }) => inspect_schedules(&database, None),
            Some(Command::Schedule {
                command: Some(ScheduleCommand::Get { database, id }),
            }) => inspect_schedule(&database, &id),
            Some(Command::Schedule {
                command: Some(ScheduleCommand::Status { database, status }),
            }) => inspect_schedules_by_status(&database, &status),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::Due {
                        database,
                        cutoff_unix_millis,
                    }),
            }) => inspect_schedules(&database, Some(cutoff_unix_millis)),
            Some(Command::Schedule {
                command: Some(ScheduleCommand::History { database, id }),
            }) => inspect_schedule_history(&database, &id),
            Some(Command::Schedule {
                command: Some(ScheduleCommand::Task { database, task_id }),
            }) => inspect_schedule_task(&database, &task_id),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Create {
                        database,
                        id,
                        goal,
                        anchor_unix_millis,
                        interval_millis,
                        occurrence_count,
                    }),
            }) => create_recurrence(
                &database,
                &id,
                &goal,
                anchor_unix_millis,
                interval_millis,
                occurrence_count,
            ),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::Inspect { database }),
            }) => inspect_recurrences(&database),
            _ => ExitCode::SUCCESS,
        }
    }
}

#[derive(Serialize)]
struct RecurrenceInventory<'a> {
    recurrences: Vec<RecurrenceInspection<'a>>,
}

#[derive(Serialize)]
struct RecurrenceInspection<'a> {
    id: &'a str,
    goal: &'a str,
    anchor_unix_millis: u64,
    interval_millis: u64,
    occurrence_count: u64,
    final_occurrence_unix_millis: u64,
    revision: u64,
}

#[derive(Serialize)]
struct ScheduleInventory<'a> {
    schedules: Vec<ScheduleInspection<'a>>,
}

#[derive(Serialize)]
struct ScheduleInspection<'a> {
    id: &'a str,
    goal: &'a str,
    due_at_unix_millis: u64,
    status: &'static str,
    revision: u64,
    cancellation: Option<&'a str>,
    latest_release: Option<&'a str>,
    task_id: Option<&'a str>,
}

#[derive(Serialize)]
struct ScheduleLookup<'a> {
    id: &'a str,
    schedule: Option<ScheduleInspection<'a>>,
}

#[derive(Serialize)]
struct NextScheduleResult<'a> {
    schedule: Option<ScheduleInspection<'a>>,
}

#[derive(Serialize)]
struct ScheduleTaskInspection<'a> {
    task_id: &'a str,
    schedule: Option<ScheduleInspection<'a>>,
}

#[derive(Serialize)]
struct ScheduleHistoryInspection<'a> {
    id: &'a str,
    history: Option<Vec<ScheduleHistoryEntryInspection<'a>>>,
}

#[derive(Serialize)]
struct ScheduleHistoryEntryInspection<'a> {
    revision: u64,
    #[serde(flatten)]
    event: ScheduleHistoryEventInspection<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ScheduleHistoryEventInspection<'a> {
    Created {
        goal: &'a str,
        due_at_unix_millis: u64,
    },
    Cancelled {
        reason: &'a str,
    },
    Claimed,
    Released {
        reason: &'a str,
    },
    Materialized {
        task_id: &'a str,
    },
}

fn create_schedule(database: &Path, raw_id: &str, raw_goal: &str, due_at: u64) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let goal = match TaskGoal::new(raw_goal) {
        Ok(goal) => goal,
        Err(error) => return extension_error("invalid_task_goal", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_creation_failed", error),
    };
    let scheduled = match store.schedule(id, goal, ScheduleInstant::from_unix_millis(due_at)) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_creation_failed", error),
    };
    let output = match serde_json::to_string(&schedule_inspection(&scheduled)) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_creation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn cancel_schedule(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    raw_reason: &str,
) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let cancellation = match ScheduleCancellation::new(raw_reason) {
        Ok(cancellation) => cancellation,
        Err(error) => return extension_error("invalid_schedule_cancellation", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_cancellation_failed", error),
    };
    let scheduled = match store.cancel(&id, expected_revision, cancellation) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_cancellation_failed", error),
    };
    let output = match serde_json::to_string(&schedule_inspection(&scheduled)) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_cancellation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn claim_schedule(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_claim_failed", error),
    };
    let scheduled = match store.claim(
        &id,
        expected_revision,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_claim_failed", error),
    };
    let output = match serde_json::to_string(&schedule_inspection(&scheduled)) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_claim_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn claim_next_schedule(database: &Path, cutoff_unix_millis: u64) -> ExitCode {
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_claim_failed", error),
    };
    let scheduled =
        match store.claim_next_due(ScheduleInstant::from_unix_millis(cutoff_unix_millis)) {
            Ok(scheduled) => scheduled,
            Err(error) => return extension_error("schedule_claim_failed", error),
        };
    let output = match serde_json::to_string(&NextScheduleResult {
        schedule: scheduled.as_ref().map(schedule_inspection),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_claim_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn release_schedule(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    raw_reason: &str,
) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let release = match ScheduleRelease::new(raw_reason) {
        Ok(release) => release,
        Err(error) => return extension_error("invalid_schedule_release_reason", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_release_failed", error),
    };
    let scheduled = match store.release(&id, expected_revision, release) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_release_failed", error),
    };
    let output = match serde_json::to_string(&schedule_inspection(&scheduled)) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_release_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_schedule(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    raw_task_id: &str,
) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    let scheduled = match store.materialize(&id, expected_revision, task_id) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    let output = match serde_json::to_string(&schedule_inspection(&scheduled)) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_next_schedule(
    database: &Path,
    cutoff_unix_millis: u64,
    raw_task_id: &str,
) -> ExitCode {
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match ScheduleStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    let scheduled = match store.materialize_next_due(
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
        task_id,
    ) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    let output = match serde_json::to_string(&NextScheduleResult {
        schedule: scheduled.as_ref().map(schedule_inspection),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_materialization_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_schedule(database: &Path, raw_id: &str) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_lookup_failed", error),
    };
    let scheduled = match store.load(&id) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_lookup_failed", error),
    };
    let output = match serde_json::to_string(&ScheduleLookup {
        id: id.as_str(),
        schedule: scheduled.as_ref().map(schedule_inspection),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_schedule_history(database: &Path, raw_id: &str) -> ExitCode {
    let id = match ScheduleId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_history_failed", error),
    };
    let history = match store.history(&id) {
        Ok(history) => history,
        Err(error) => return extension_error("schedule_history_failed", error),
    };
    let history = match history.as_ref() {
        None => None,
        Some(entries) => {
            let mut output = Vec::with_capacity(entries.len());
            for entry in entries {
                let event = match entry.event() {
                    ScheduleHistoryEvent::Created { goal, due_at } => {
                        ScheduleHistoryEventInspection::Created {
                            goal: goal.as_str(),
                            due_at_unix_millis: due_at.unix_millis(),
                        }
                    }
                    ScheduleHistoryEvent::Cancelled { reason } => {
                        ScheduleHistoryEventInspection::Cancelled {
                            reason: reason.as_str(),
                        }
                    }
                    ScheduleHistoryEvent::Claimed => ScheduleHistoryEventInspection::Claimed,
                    ScheduleHistoryEvent::Released { reason } => {
                        ScheduleHistoryEventInspection::Released {
                            reason: reason.as_str(),
                        }
                    }
                    ScheduleHistoryEvent::Materialized { task_id } => {
                        ScheduleHistoryEventInspection::Materialized {
                            task_id: task_id.as_str(),
                        }
                    }
                    _ => {
                        return extension_error(
                            "schedule_history_failed",
                            "unsupported schedule history event",
                        );
                    }
                };
                output.push(ScheduleHistoryEntryInspection {
                    revision: entry.revision(),
                    event,
                });
            }
            Some(output)
        }
    };
    let output = match serde_json::to_string(&ScheduleHistoryInspection {
        id: id.as_str(),
        history,
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_history_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_schedules(database: &Path, cutoff_unix_millis: Option<u64>) -> ExitCode {
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_inspection_failed", error),
    };
    let schedules = match cutoff_unix_millis.map_or_else(
        || store.list(),
        |cutoff| store.list_due(ScheduleInstant::from_unix_millis(cutoff)),
    ) {
        Ok(schedules) => schedules,
        Err(error) => return extension_error("schedule_inspection_failed", error),
    };
    write_schedule_inventory(&schedules, "schedule_inspection_failed")
}

fn create_recurrence(
    database: &Path,
    raw_id: &str,
    raw_goal: &str,
    anchor_unix_millis: u64,
    interval_millis: u64,
    occurrence_count: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let goal = match TaskGoal::new(raw_goal) {
        Ok(goal) => goal,
        Err(error) => return extension_error("invalid_task_goal", error),
    };
    let interval = match ScheduleInterval::from_millis(interval_millis) {
        Ok(interval) => interval,
        Err(error) => return extension_error("invalid_recurrence_interval", error),
    };
    let occurrence_count = match OccurrenceCount::new(occurrence_count) {
        Ok(count) => count,
        Err(error) => return extension_error("invalid_occurrence_count", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_creation_failed", error),
    };
    let recurrence = match store.create(
        id,
        goal,
        ScheduleInstant::from_unix_millis(anchor_unix_millis),
        interval,
        occurrence_count,
    ) {
        Ok(recurrence) => recurrence,
        Err(error) => return extension_error("recurrence_creation_failed", error),
    };
    let output = match serde_json::to_string(&recurrence_inspection(&recurrence)) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_creation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_recurrences(database: &Path) -> ExitCode {
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_inspection_failed", error),
    };
    let recurrences = match store.list() {
        Ok(recurrences) => recurrences,
        Err(error) => return extension_error("recurrence_inspection_failed", error),
    };
    let inventory = RecurrenceInventory {
        recurrences: recurrences.iter().map(recurrence_inspection).collect(),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_inspection_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn recurrence_inspection(recurrence: &FixedIntervalRecurrence) -> RecurrenceInspection<'_> {
    RecurrenceInspection {
        id: recurrence.id().as_str(),
        goal: recurrence.goal().as_str(),
        anchor_unix_millis: recurrence.anchor().unix_millis(),
        interval_millis: recurrence.interval().millis(),
        occurrence_count: recurrence.occurrence_count().get(),
        final_occurrence_unix_millis: recurrence.final_occurrence().unix_millis(),
        revision: recurrence.revision(),
    }
}

fn inspect_schedules_by_status(database: &Path, raw_status: &str) -> ExitCode {
    let status = match raw_status {
        "pending" => ScheduleStatus::Pending,
        "cancelled" => ScheduleStatus::Cancelled,
        "claimed" => ScheduleStatus::Claimed,
        "materialized" => ScheduleStatus::Materialized,
        _ => {
            return extension_error(
                "invalid_schedule_status",
                "expected pending, cancelled, claimed, or materialized",
            );
        }
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_status_inspection_failed", error),
    };
    let schedules = match store.list_by_status(status) {
        Ok(schedules) => schedules,
        Err(error) => return extension_error("schedule_status_inspection_failed", error),
    };
    write_schedule_inventory(&schedules, "schedule_status_inspection_failed")
}

fn write_schedule_inventory(schedules: &[ScheduledTask], error_code: &str) -> ExitCode {
    let inventory = ScheduleInventory {
        schedules: schedules.iter().map(schedule_inspection).collect(),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error(error_code, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_schedule_task(database: &Path, raw_task_id: &str) -> ExitCode {
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_task_lookup_failed", error),
    };
    let scheduled = match store.find_by_task_id(&task_id) {
        Ok(scheduled) => scheduled,
        Err(error) => return extension_error("schedule_task_lookup_failed", error),
    };
    let output = match serde_json::to_string(&ScheduleTaskInspection {
        task_id: task_id.as_str(),
        schedule: scheduled.as_ref().map(schedule_inspection),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_task_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn schedule_inspection(scheduled: &ScheduledTask) -> ScheduleInspection<'_> {
    ScheduleInspection {
        id: scheduled.id().as_str(),
        goal: scheduled.goal().as_str(),
        due_at_unix_millis: scheduled.due_at().unix_millis(),
        status: match scheduled.status() {
            ScheduleStatus::Pending => "pending",
            ScheduleStatus::Cancelled => "cancelled",
            ScheduleStatus::Claimed => "claimed",
            ScheduleStatus::Materialized => "materialized",
        },
        revision: scheduled.revision(),
        cancellation: scheduled.cancellation().map(|reason| reason.as_str()),
        latest_release: scheduled.latest_release().map(|reason| reason.as_str()),
        task_id: scheduled.task_id().map(|id| id.as_str()),
    }
}

fn invoke_extension(root: &Path, id: &str, input_json: &str) -> ExitCode {
    let input = match serde_json::from_str(input_json) {
        Ok(input) => input,
        Err(error) => return extension_error("invalid_tool_input", error),
    };
    let extensions = match ExtensionRegistry::discover(root) {
        Ok(extensions) => extensions,
        Err(error) => return extension_error("invalid_extension_root", error),
    };
    let selection = match extensions.select_kind(ExtensionKind::Tool, [id]) {
        Ok(selection) => selection,
        Err(error) => return extension_error("invalid_tool_selection", error),
    };
    let mut tools = ToolRegistry::new();
    if let Err(error) = activate_tool_selection(root, &selection, &mut tools) {
        return extension_error("tool_activation_failed", error);
    }
    let tool_id = match ToolId::new(id) {
        Ok(tool_id) => tool_id,
        Err(error) => return extension_error("tool_activation_failed", error),
    };
    let mut authorizer = OneShotPureAuthorization {
        tool_id: tool_id.clone(),
        available: true,
    };
    let output = match tools.invoke(&tool_id, &mut authorizer, &input) {
        Ok(output) => output,
        Err(error) => return extension_error("tool_invocation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

struct OneShotPureAuthorization {
    tool_id: ToolId,
    available: bool,
}

impl ToolAuthorizer for OneShotPureAuthorization {
    fn authorize(&mut self, request: ToolRequest<'_>) -> PermissionDecision {
        if self.available
            && request.tool_id() == &self.tool_id
            && request.effect() == ToolEffect::Pure
        {
            self.available = false;
            PermissionDecision::Allow
        } else {
            PermissionDecision::Deny
        }
    }
}

fn extension_error(code: &str, error: impl std::fmt::Display) -> ExitCode {
    let diagnostic = error.to_string();
    eprintln!("$: {code}: {diagnostic:?}");
    ExitCode::from(1)
}

fn inspect_extensions(root: &Path) -> ExitCode {
    let registry = match ExtensionRegistry::discover(root) {
        Ok(registry) => registry,
        Err(error) => return extension_error("invalid_extension_root", error),
    };

    for extension in registry.extensions() {
        let manifest = extension.manifest();
        let kind = match manifest.kind() {
            ExtensionKind::Tool => "tool",
            ExtensionKind::Skill => "skill",
            ExtensionKind::Workflow => "workflow",
        };
        let path = extension
            .path()
            .strip_prefix(root)
            .unwrap_or(extension.path());
        println!(
            "{:?}\t{kind}\t{:?}\t{path:?}",
            manifest.id(),
            manifest.entrypoint()
        );
    }
    println!("inspected {} extensions", registry.extensions().len());
    ExitCode::SUCCESS
}

fn inspect_corpus(root: &Path) -> ExitCode {
    let mut paths = Vec::new();
    if let Err(error) = collect_json(root, &mut paths) {
        eprintln!("$: unreadable_corpus: {error}");
        return ExitCode::from(2);
    }
    paths.sort();

    let mut valid = 0;
    for path in &paths {
        let relative = path.strip_prefix(root).unwrap_or(path).display();
        let input = match fs::read_to_string(path) {
            Ok(input) => input,
            Err(error) => {
                eprintln!("{relative}: unreadable_record: {error}");
                continue;
            }
        };
        let record: DevelopmentRecord = match serde_json::from_str(&input) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("{relative}: malformed_record: {error}");
                continue;
            }
        };
        let issues = record.validate();
        if issues.is_empty() {
            println!("{relative}: valid");
            valid += 1;
        } else {
            for issue in issues {
                eprintln!(
                    "{relative}: {}: {}: {}",
                    issue.path, issue.code, issue.message
                );
            }
        }
    }

    let invalid = paths.len() - valid;
    println!(
        "inspected {} records: {valid} valid, {invalid} invalid",
        paths.len()
    );
    if invalid == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn collect_json(directory: &Path, paths: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_json(&path, paths)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    Ok(())
}

fn validate_record(path: &Path) -> ExitCode {
    let input = match fs::read_to_string(path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("$: unreadable_record: {error}");
            return ExitCode::from(2);
        }
    };
    let record: DevelopmentRecord = match serde_json::from_str(&input) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("$: malformed_record: {error}");
            return ExitCode::from(2);
        }
    };
    let issues = record.validate();
    if issues.is_empty() {
        println!("valid development record: {}", path.display());
        ExitCode::SUCCESS
    } else {
        for issue in issues {
            eprintln!("{}: {}: {}", issue.path, issue.code, issue.message);
        }
        ExitCode::from(1)
    }
}
