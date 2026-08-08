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
    AvailableRecurrenceOccurrence, AvailableRecurrenceOccurrencePage, ClaimedRecurrenceOccurrence,
    ClaimedRecurrenceOccurrencePage, FixedIntervalRecurrence, MaterializedRecurrenceOccurrence,
    MaterializedRecurrenceOccurrencePage, OccurrenceCount, OccurrencePageSize,
    RecurrenceCancellation, RecurrenceHistoryEvent, RecurrenceId, RecurrenceOccurrence,
    RecurrenceOccurrenceHistoryEntry, RecurrenceOccurrenceHistoryEvent,
    RecurrenceOccurrenceLookupError, RecurrenceOccurrencePage, RecurrenceOccurrenceRelease,
    RecurrencePageSize, RecurrenceStatus, RecurrenceStore, RecurrenceStoreError,
    ScheduleCancellation, ScheduleHistoryEvent, ScheduleId, ScheduleInstant, ScheduleInterval,
    SchedulePageSize, ScheduleRelease, ScheduleStatus, ScheduleStore, ScheduledTask,
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
    /// Cancel one exact recurrence aggregate revision.
    Cancel {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        reason: String,
    },
    /// Print one finite recurrence selected by exact ID through a read-only boundary.
    Get { database: PathBuf, id: String },
    /// Print one exact recurrence's validated lifecycle history.
    History { database: PathBuf, id: String },
    /// Print one exact persisted recurrence occurrence's validated lifecycle history.
    OccurrenceHistory {
        database: PathBuf,
        id: String,
        offset: u64,
    },
    /// Page complete persisted occurrence histories through a read-only boundary.
    OccurrenceHistories {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Page exact occurrences for one finite recurrence through a read-only boundary.
    Occurrences {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Page due occurrences through one caller-owned inclusive cutoff.
    Due {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
        cutoff_unix_millis: u64,
    },
    /// Select the latest due occurrence through one caller-owned inclusive cutoff.
    LatestDue {
        database: PathBuf,
        id: String,
        start_offset: u64,
        cutoff_unix_millis: u64,
    },
    /// Atomically persist the latest due occurrence through one inclusive cutoff.
    PersistLatestDue {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        start_offset: u64,
        cutoff_unix_millis: u64,
    },
    /// Atomically bind the latest due occurrence to a caller-owned inert task.
    MaterializeLatestDue {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        start_offset: u64,
        cutoff_unix_millis: u64,
        task_id: String,
    },
    /// Atomically persist one bounded page through a caller-owned inclusive cutoff.
    PersistDue {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        start_offset: u64,
        page_size: u64,
        cutoff_unix_millis: u64,
    },
    /// Atomically bind one bounded due page to caller-owned inert tasks.
    MaterializeDue {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        start_offset: u64,
        page_size: u64,
        cutoff_unix_millis: u64,
        task_ids: Vec<String>,
    },
    /// Page persisted provenance for one finite recurrence through a read-only boundary.
    Persisted {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Page current claims for one recurrence through a read-only boundary.
    Claimed {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Page currently available occurrences for one recurrence through a read-only boundary.
    Available {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Claim the earliest available due occurrence in one caller-selected window.
    ClaimNext {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
        cutoff_unix_millis: u64,
    },
    /// Atomically bind the earliest available due occurrence in one selected window.
    MaterializeNext {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
        cutoff_unix_millis: u64,
        task_id: String,
    },
    /// Page materialized task bindings for one recurrence through a read-only boundary.
    Materialized {
        database: PathBuf,
        id: String,
        start_offset: u64,
        page_size: u64,
    },
    /// Print one exact persisted occurrence through a read-only boundary.
    Occurrence {
        database: PathBuf,
        id: String,
        offset: u64,
    },
    /// Persist provenance for one exact authored recurrence occurrence.
    Persist {
        database: PathBuf,
        id: String,
        expected_revision: u64,
        offset: u64,
    },
    /// Claim one exact persisted occurrence against a caller-owned cutoff.
    Claim {
        database: PathBuf,
        id: String,
        offset: u64,
        expected_occurrence_revision: u64,
        cutoff_unix_millis: u64,
    },
    /// Release one exact claimed occurrence revision with recovery evidence.
    Release {
        database: PathBuf,
        id: String,
        offset: u64,
        expected_occurrence_revision: u64,
        reason: String,
    },
    /// Atomically bind one persisted occurrence to a caller-owned inert task.
    Materialize {
        database: PathBuf,
        id: String,
        offset: u64,
        expected_occurrence_revision: u64,
        task_id: String,
    },
    /// Atomically bind one claimed occurrence to a caller-owned inert task.
    MaterializeClaimed {
        database: PathBuf,
        id: String,
        offset: u64,
        expected_occurrence_revision: u64,
        task_id: String,
    },
    /// Resolve one materialized occurrence from an exact task identity.
    Task { database: PathBuf, task_id: String },
    /// Print every finite recurrence through a read-only storage boundary.
    Inspect { database: PathBuf },
    /// Page finite recurrences through a bounded read-only storage boundary.
    Page {
        database: PathBuf,
        page_size: u64,
        after: Option<String>,
    },
    /// Page finite recurrences sparsely by lifecycle status through a bounded read-only scan.
    StatusPage {
        database: PathBuf,
        status: String,
        scan_size: u64,
        after: Option<String>,
    },
    /// Print finite recurrences with one exact lifecycle status.
    Status { database: PathBuf, status: String },
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
    /// Page durable schedules through a bounded read-only storage boundary.
    Page {
        database: PathBuf,
        page_size: u64,
        after: Option<String>,
    },
    /// Page schedules sparsely by lifecycle status through a bounded read-only scan.
    StatusPage {
        database: PathBuf,
        status: String,
        scan_size: u64,
        after: Option<String>,
    },
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
                command:
                    Some(ScheduleCommand::Page {
                        database,
                        page_size,
                        after,
                    }),
            }) => page_schedules(&database, page_size, after.as_deref()),
            Some(Command::Schedule {
                command:
                    Some(ScheduleCommand::StatusPage {
                        database,
                        status,
                        scan_size,
                        after,
                    }),
            }) => page_schedules_by_status(&database, &status, scan_size, after.as_deref()),
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
                command:
                    Some(RecurrenceCommand::Cancel {
                        database,
                        id,
                        expected_revision,
                        reason,
                    }),
            }) => cancel_recurrence(&database, &id, expected_revision, &reason),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::Get { database, id }),
            }) => get_recurrence(&database, &id),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::History { database, id }),
            }) => inspect_recurrence_history(&database, &id),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::OccurrenceHistory {
                        database,
                        id,
                        offset,
                    }),
            }) => inspect_recurrence_occurrence_history(&database, &id, offset),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::OccurrenceHistories {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_recurrence_occurrence_histories(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Occurrences {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_recurrence_occurrences(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Due {
                        database,
                        id,
                        start_offset,
                        page_size,
                        cutoff_unix_millis,
                    }),
            }) => page_due_recurrence_occurrences(
                &database,
                &id,
                start_offset,
                page_size,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::LatestDue {
                        database,
                        id,
                        start_offset,
                        cutoff_unix_millis,
                    }),
            }) => select_latest_due_recurrence_occurrence(
                &database,
                &id,
                start_offset,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::PersistLatestDue {
                        database,
                        id,
                        expected_revision,
                        start_offset,
                        cutoff_unix_millis,
                    }),
            }) => persist_latest_due_recurrence_occurrence(
                &database,
                &id,
                expected_revision,
                start_offset,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::MaterializeLatestDue {
                        database,
                        id,
                        expected_revision,
                        start_offset,
                        cutoff_unix_millis,
                        task_id,
                    }),
            }) => materialize_latest_due_recurrence_occurrence(
                &database,
                &id,
                expected_revision,
                start_offset,
                cutoff_unix_millis,
                &task_id,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::PersistDue {
                        database,
                        id,
                        expected_revision,
                        start_offset,
                        page_size,
                        cutoff_unix_millis,
                    }),
            }) => persist_due_recurrence_occurrences(
                &database,
                &id,
                expected_revision,
                start_offset,
                page_size,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::MaterializeDue {
                        database,
                        id,
                        expected_revision,
                        start_offset,
                        page_size,
                        cutoff_unix_millis,
                        task_ids,
                    }),
            }) => materialize_due_recurrence_occurrences(
                &database,
                &id,
                expected_revision,
                start_offset,
                page_size,
                cutoff_unix_millis,
                &task_ids,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Persisted {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_persisted_recurrence_occurrences(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Claimed {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_claimed_recurrence_occurrences(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Available {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_available_recurrence_occurrences(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::ClaimNext {
                        database,
                        id,
                        start_offset,
                        page_size,
                        cutoff_unix_millis,
                    }),
            }) => claim_next_recurrence_occurrence(
                &database,
                &id,
                start_offset,
                page_size,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::MaterializeNext {
                        database,
                        id,
                        start_offset,
                        page_size,
                        cutoff_unix_millis,
                        task_id,
                    }),
            }) => materialize_next_recurrence_occurrence(
                &database,
                &id,
                start_offset,
                page_size,
                cutoff_unix_millis,
                &task_id,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Materialized {
                        database,
                        id,
                        start_offset,
                        page_size,
                    }),
            }) => page_materialized_recurrence_occurrences(&database, &id, start_offset, page_size),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Occurrence {
                        database,
                        id,
                        offset,
                    }),
            }) => get_recurrence_occurrence(&database, &id, offset),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Persist {
                        database,
                        id,
                        expected_revision,
                        offset,
                    }),
            }) => persist_recurrence_occurrence(&database, &id, expected_revision, offset),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Claim {
                        database,
                        id,
                        offset,
                        expected_occurrence_revision,
                        cutoff_unix_millis,
                    }),
            }) => claim_recurrence_occurrence(
                &database,
                &id,
                offset,
                expected_occurrence_revision,
                cutoff_unix_millis,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Release {
                        database,
                        id,
                        offset,
                        expected_occurrence_revision,
                        reason,
                    }),
            }) => release_recurrence_occurrence(
                &database,
                &id,
                offset,
                expected_occurrence_revision,
                &reason,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Materialize {
                        database,
                        id,
                        offset,
                        expected_occurrence_revision,
                        task_id,
                    }),
            }) => materialize_recurrence_occurrence(
                &database,
                &id,
                offset,
                expected_occurrence_revision,
                &task_id,
            ),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::MaterializeClaimed {
                        database,
                        id,
                        offset,
                        expected_occurrence_revision,
                        task_id,
                    }),
            }) => materialize_claimed_recurrence_occurrence(
                &database,
                &id,
                offset,
                expected_occurrence_revision,
                &task_id,
            ),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::Task { database, task_id }),
            }) => inspect_recurrence_task(&database, &task_id),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::Inspect { database }),
            }) => inspect_recurrences(&database),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::Page {
                        database,
                        page_size,
                        after,
                    }),
            }) => page_recurrences(&database, page_size, after.as_deref()),
            Some(Command::Recurrence {
                command:
                    Some(RecurrenceCommand::StatusPage {
                        database,
                        status,
                        scan_size,
                        after,
                    }),
            }) => page_recurrences_by_status(&database, &status, scan_size, after.as_deref()),
            Some(Command::Recurrence {
                command: Some(RecurrenceCommand::Status { database, status }),
            }) => inspect_recurrences_by_status(&database, &status),
            _ => ExitCode::SUCCESS,
        }
    }
}

#[derive(Serialize)]
struct RecurrenceInventory<'a> {
    recurrences: Vec<RecurrenceInspection<'a>>,
}

#[derive(Serialize)]
struct RecurrenceInventoryPage<'a> {
    recurrences: Vec<RecurrenceInspection<'a>>,
    next_after: Option<&'a str>,
}

#[derive(Serialize)]
struct RecurrenceInspection<'a> {
    id: &'a str,
    goal: &'a str,
    anchor_unix_millis: u64,
    interval_millis: u64,
    occurrence_count: u64,
    status: &'static str,
    final_occurrence_unix_millis: u64,
    definition_revision: u64,
    aggregate_revision: u64,
    cancellation: Option<&'a str>,
}

#[derive(Serialize)]
struct RecurrenceHistoryInspection<'a> {
    id: &'a str,
    history: Option<Vec<RecurrenceHistoryEntryInspection<'a>>>,
}

#[derive(Serialize)]
struct RecurrenceHistoryEntryInspection<'a> {
    revision: u64,
    #[serde(flatten)]
    event: RecurrenceHistoryEventInspection<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecurrenceHistoryEventInspection<'a> {
    Created {
        goal: &'a str,
        anchor_unix_millis: u64,
        interval_millis: u64,
        occurrence_count: u64,
    },
    Cancelled {
        reason: &'a str,
    },
}

#[derive(Serialize)]
struct RecurrenceOccurrenceHistoryInspection<'a> {
    recurrence_id: &'a str,
    offset: u64,
    history: Option<Vec<RecurrenceOccurrenceHistoryEntryInspection<'a>>>,
}

#[derive(Serialize)]
struct RecurrenceOccurrenceHistoryPageInspection<'a> {
    histories: Vec<RecurrenceOccurrenceHistoryInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct RecurrenceOccurrenceHistoryEntryInspection<'a> {
    revision: u64,
    #[serde(flatten)]
    event: RecurrenceOccurrenceHistoryEventInspection<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RecurrenceOccurrenceHistoryEventInspection<'a> {
    Persisted {
        goal: &'a str,
        unix_millis: u64,
        definition_revision: u64,
    },
    Claimed,
    Released {
        reason: &'a str,
    },
    Materialized {
        task_id: &'a str,
    },
}

#[derive(Serialize)]
struct RecurrenceOccurrencePageInspection<'a> {
    occurrences: Vec<RecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct LatestDueOccurrenceInspection<'a> {
    occurrence: Option<RecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct LatestDueMaterializationInspection<'a> {
    occurrence: Option<MaterializedRecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct RecurrenceOccurrenceInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
}

#[derive(Serialize)]
struct ClaimedRecurrenceOccurrenceInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
    occurrence_revision: u64,
}

#[derive(Serialize)]
struct ClaimedRecurrenceOccurrencePageInspection<'a> {
    occurrences: Vec<ClaimedRecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct ClaimNextRecurrenceOccurrenceInspection<'a> {
    occurrence: Option<ClaimedRecurrenceOccurrenceWithReleaseInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct MaterializeNextRecurrenceOccurrenceInspection<'a> {
    occurrence: Option<MaterializedRecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct ClaimedRecurrenceOccurrenceWithReleaseInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
    occurrence_revision: u64,
    latest_release: Option<&'a str>,
}

#[derive(Serialize)]
struct AvailableRecurrenceOccurrenceInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
    occurrence_revision: u64,
    latest_release: Option<&'a str>,
}

#[derive(Serialize)]
struct AvailableRecurrenceOccurrencePageInspection<'a> {
    occurrences: Vec<AvailableRecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct ReleasedRecurrenceOccurrenceInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
    occurrence_revision: u64,
    latest_release: &'a str,
}

#[derive(Serialize)]
struct MaterializedRecurrenceOccurrenceInspection<'a> {
    recurrence_id: &'a str,
    goal: &'a str,
    offset: u64,
    unix_millis: u64,
    definition_revision: u64,
    occurrence_revision: u64,
    task_id: &'a str,
}

#[derive(Serialize)]
struct MaterializedRecurrenceOccurrencePageInspection<'a> {
    occurrences: Vec<MaterializedRecurrenceOccurrenceInspection<'a>>,
    next_offset: Option<u64>,
}

#[derive(Serialize)]
struct RecurrenceTaskInspection<'a> {
    task_id: &'a str,
    occurrence: Option<MaterializedRecurrenceOccurrenceInspection<'a>>,
}

#[derive(Serialize)]
struct ScheduleInventory<'a> {
    schedules: Vec<ScheduleInspection<'a>>,
}

#[derive(Serialize)]
struct ScheduleInventoryPage<'a> {
    schedules: Vec<ScheduleInspection<'a>>,
    next_after: Option<&'a str>,
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

fn page_schedules(database: &Path, raw_page_size: u64, raw_after: Option<&str>) -> ExitCode {
    let page_size = match SchedulePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_schedule_page_size", error),
    };
    let after = match raw_after.map(ScheduleId::new).transpose() {
        Ok(after) => after,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_page_inspection_failed", error),
    };
    let page = match store.list_page(after.as_ref(), page_size) {
        Ok(page) => page,
        Err(error) => return extension_error("schedule_page_inspection_failed", error),
    };
    let inventory = ScheduleInventoryPage {
        schedules: page.schedules().iter().map(schedule_inspection).collect(),
        next_after: page.next_after().map(ScheduleId::as_str),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_page_inspection_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_schedules_by_status(
    database: &Path,
    raw_status: &str,
    raw_scan_size: u64,
    raw_after: Option<&str>,
) -> ExitCode {
    let status = match parse_schedule_status(raw_status) {
        Ok(status) => status,
        Err(error) => return extension_error("invalid_schedule_status", error),
    };
    let scan_size = match SchedulePageSize::new(raw_scan_size) {
        Ok(scan_size) => scan_size,
        Err(error) => return extension_error("invalid_schedule_page_size", error),
    };
    let after = match raw_after.map(ScheduleId::new).transpose() {
        Ok(after) => after,
        Err(error) => return extension_error("invalid_schedule_id", error),
    };
    let store = match ScheduleStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("schedule_status_page_inspection_failed", error),
    };
    let page = match store.list_by_status_page(status, after.as_ref(), scan_size) {
        Ok(page) => page,
        Err(error) => return extension_error("schedule_status_page_inspection_failed", error),
    };
    let inventory = ScheduleInventoryPage {
        schedules: page.schedules().iter().map(schedule_inspection).collect(),
        next_after: page.next_after().map(ScheduleId::as_str),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error("schedule_status_page_inspection_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
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

fn cancel_recurrence(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    raw_reason: &str,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let cancellation = match RecurrenceCancellation::new(raw_reason) {
        Ok(cancellation) => cancellation,
        Err(error) => return extension_error("invalid_recurrence_cancellation", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_cancellation_failed", error),
    };
    let recurrence = match store.cancel(&id, expected_revision, cancellation) {
        Ok(recurrence) => recurrence,
        Err(error) => return extension_error("recurrence_cancellation_failed", error),
    };
    let output = match serde_json::to_string(&recurrence_inspection(&recurrence)) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_cancellation_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn get_recurrence(database: &Path, raw_id: &str) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let recurrence = match load_recurrence(database, &id, "recurrence_lookup_failed") {
        Ok(recurrence) => recurrence,
        Err(exit_code) => return exit_code,
    };
    let output = match serde_json::to_string(&recurrence_inspection(&recurrence)) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_recurrence_history(database: &Path, raw_id: &str) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_history_failed", error),
    };
    let history = match store.history(&id) {
        Ok(history) => history,
        Err(error) => return extension_error("recurrence_history_failed", error),
    };
    let history = match history.as_ref() {
        None => None,
        Some(entries) => {
            let mut output = Vec::with_capacity(entries.len());
            for entry in entries {
                let event = match entry.event() {
                    RecurrenceHistoryEvent::Created {
                        goal,
                        anchor,
                        interval,
                        occurrence_count,
                    } => RecurrenceHistoryEventInspection::Created {
                        goal: goal.as_str(),
                        anchor_unix_millis: anchor.unix_millis(),
                        interval_millis: interval.millis(),
                        occurrence_count: occurrence_count.get(),
                    },
                    RecurrenceHistoryEvent::Cancelled { reason } => {
                        RecurrenceHistoryEventInspection::Cancelled {
                            reason: reason.as_str(),
                        }
                    }
                    _ => {
                        return extension_error(
                            "recurrence_history_failed",
                            "unsupported recurrence history event",
                        );
                    }
                };
                output.push(RecurrenceHistoryEntryInspection {
                    revision: entry.revision(),
                    event,
                });
            }
            Some(output)
        }
    };
    let output = match serde_json::to_string(&RecurrenceHistoryInspection {
        id: id.as_str(),
        history,
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_history_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_recurrence_occurrence_history(database: &Path, raw_id: &str, offset: u64) -> ExitCode {
    const ERROR: &str = "recurrence_occurrence_history_failed";
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error(ERROR, error),
    };
    let history = match store.occurrence_history(&id, offset) {
        Ok(history) => history,
        Err(error) => return extension_error(ERROR, error),
    };
    let history = match history.as_deref() {
        None => None,
        Some(entries) => match inspect_recurrence_occurrence_history_entries(entries) {
            Ok(entries) => Some(entries),
            Err(error) => return extension_error(ERROR, error),
        },
    };
    let output = match serde_json::to_string(&RecurrenceOccurrenceHistoryInspection {
        recurrence_id: id.as_str(),
        offset,
        history,
    }) {
        Ok(output) => output,
        Err(error) => return extension_error(ERROR, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_recurrence_occurrence_histories(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    const ERROR: &str = "recurrence_occurrence_histories_failed";
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error(ERROR, error),
    };
    let page = match store.occurrence_histories_page(&id, start_offset, page_size) {
        Ok(page) => page,
        Err(error) => return extension_error(ERROR, error),
    };
    let mut histories = Vec::with_capacity(page.histories().len());
    for history in page.histories() {
        let entries = match inspect_recurrence_occurrence_history_entries(history.entries()) {
            Ok(entries) => entries,
            Err(error) => return extension_error(ERROR, error),
        };
        histories.push(RecurrenceOccurrenceHistoryInspection {
            recurrence_id: id.as_str(),
            offset: history.offset(),
            history: Some(entries),
        });
    }
    let output = match serde_json::to_string(&RecurrenceOccurrenceHistoryPageInspection {
        histories,
        next_offset: page.next_offset(),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error(ERROR, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_recurrence_occurrence_history_entries(
    entries: &[RecurrenceOccurrenceHistoryEntry],
) -> Result<Vec<RecurrenceOccurrenceHistoryEntryInspection<'_>>, &'static str> {
    let mut output = Vec::with_capacity(entries.len());
    for entry in entries {
        let event = match entry.event() {
            RecurrenceOccurrenceHistoryEvent::Persisted { occurrence } => {
                RecurrenceOccurrenceHistoryEventInspection::Persisted {
                    goal: occurrence.goal().as_str(),
                    unix_millis: occurrence.instant().unix_millis(),
                    definition_revision: occurrence.recurrence_revision(),
                }
            }
            RecurrenceOccurrenceHistoryEvent::Claimed => {
                RecurrenceOccurrenceHistoryEventInspection::Claimed
            }
            RecurrenceOccurrenceHistoryEvent::Released { reason } => {
                RecurrenceOccurrenceHistoryEventInspection::Released {
                    reason: reason.as_str(),
                }
            }
            RecurrenceOccurrenceHistoryEvent::Materialized { task_id } => {
                RecurrenceOccurrenceHistoryEventInspection::Materialized {
                    task_id: task_id.as_str(),
                }
            }
            _ => return Err("unsupported recurrence occurrence history event"),
        };
        output.push(RecurrenceOccurrenceHistoryEntryInspection {
            revision: entry.revision(),
            event,
        });
    }
    Ok(output)
}

fn page_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let recurrence = match load_recurrence(database, &id, "recurrence_occurrence_lookup_failed") {
        Ok(recurrence) => recurrence,
        Err(exit_code) => return exit_code,
    };
    let page = match recurrence.occurrences_page(start_offset, page_size) {
        Ok(page) => page,
        Err(error @ RecurrenceOccurrenceLookupError::OutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => return extension_error("recurrence_occurrence_lookup_failed", error),
    };
    let output = match serialize_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_occurrence_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_due_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("due_recurrence_occurrence_lookup_failed", error),
    };
    let page = match store.due_occurrences_page(
        &id,
        start_offset,
        page_size,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(page) => page,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => return extension_error("due_recurrence_occurrence_lookup_failed", error),
    };
    let output = match serialize_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => return extension_error("due_recurrence_occurrence_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn persist_due_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    start_offset: u64,
    raw_page_size: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_persistence_failed", error);
        }
    };
    let page = match store.persist_due_occurrences_page(
        &id,
        expected_revision,
        start_offset,
        page_size,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(page) => page,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_persistence_failed", error);
        }
    };
    let output = match serialize_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_persistence_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_due_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    start_offset: u64,
    raw_page_size: u64,
    cutoff_unix_millis: u64,
    raw_task_ids: &[String],
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let mut task_ids = Vec::with_capacity(raw_task_ids.len());
    for raw_task_id in raw_task_ids {
        match TaskId::new(raw_task_id) {
            Ok(task_id) => task_ids.push(task_id),
            Err(error) => return extension_error("invalid_task_id", error),
        }
    }
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_materialization_failed", error);
        }
    };
    let page = match store.materialize_due_occurrences_page(
        &id,
        expected_revision,
        start_offset,
        page_size,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
        task_ids,
    ) {
        Ok(page) => page,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_materialization_failed", error);
        }
    };
    let output = match serialize_materialized_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("due_recurrence_occurrence_materialization_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_persisted_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("persisted_recurrence_occurrence_lookup_failed", error);
        }
    };
    let page = match store.persisted_occurrences_page(&id, start_offset, page_size) {
        Ok(page) => page,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => {
            return extension_error("persisted_recurrence_occurrence_lookup_failed", error);
        }
    };
    let output = match serialize_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("persisted_recurrence_occurrence_lookup_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_claimed_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("claimed_recurrence_occurrence_lookup_failed", error);
        }
    };
    let page = match store.claimed_occurrences_page(&id, start_offset, page_size) {
        Ok(page) => page,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => {
            return extension_error("claimed_recurrence_occurrence_lookup_failed", error);
        }
    };
    let output = match serialize_claimed_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("claimed_recurrence_occurrence_lookup_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn serialize_claimed_recurrence_occurrence_page(
    page: &ClaimedRecurrenceOccurrencePage,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ClaimedRecurrenceOccurrencePageInspection {
        occurrences: page
            .occurrences()
            .iter()
            .map(claimed_recurrence_occurrence_inspection)
            .collect(),
        next_offset: page.next_offset(),
    })
}

fn claimed_recurrence_occurrence_inspection(
    claimed: &ClaimedRecurrenceOccurrence,
) -> ClaimedRecurrenceOccurrenceInspection<'_> {
    let occurrence = claimed.occurrence();
    ClaimedRecurrenceOccurrenceInspection {
        recurrence_id: occurrence.recurrence_id().as_str(),
        goal: occurrence.goal().as_str(),
        offset: occurrence.offset(),
        unix_millis: occurrence.instant().unix_millis(),
        definition_revision: occurrence.recurrence_revision(),
        occurrence_revision: claimed.revision(),
    }
}

fn page_available_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("available_recurrence_occurrence_lookup_failed", error);
        }
    };
    let page = match store.available_occurrences_page(&id, start_offset, page_size) {
        Ok(page) => page,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => {
            return extension_error("available_recurrence_occurrence_lookup_failed", error);
        }
    };
    let output = match serialize_available_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("available_recurrence_occurrence_lookup_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn serialize_available_recurrence_occurrence_page(
    page: &AvailableRecurrenceOccurrencePage,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&AvailableRecurrenceOccurrencePageInspection {
        occurrences: page
            .occurrences()
            .iter()
            .map(available_recurrence_occurrence_inspection)
            .collect(),
        next_offset: page.next_offset(),
    })
}

fn available_recurrence_occurrence_inspection(
    available: &AvailableRecurrenceOccurrence,
) -> AvailableRecurrenceOccurrenceInspection<'_> {
    let occurrence = available.occurrence();
    AvailableRecurrenceOccurrenceInspection {
        recurrence_id: occurrence.recurrence_id().as_str(),
        goal: occurrence.goal().as_str(),
        offset: occurrence.offset(),
        unix_millis: occurrence.instant().unix_millis(),
        definition_revision: occurrence.recurrence_revision(),
        occurrence_revision: available.revision(),
        latest_release: available.latest_release().map(|release| release.as_str()),
    }
}

fn claim_next_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    const FAILURE_CODE: &str = "recurrence_occurrence_claim_next_failed";
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let selection = match store.claim_next_available_occurrence(
        &id,
        start_offset,
        page_size,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(selection) => selection,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let occurrence = selection.occurrence().map(|claimed| {
        let occurrence = claimed.occurrence();
        ClaimedRecurrenceOccurrenceWithReleaseInspection {
            recurrence_id: occurrence.recurrence_id().as_str(),
            goal: occurrence.goal().as_str(),
            offset: occurrence.offset(),
            unix_millis: occurrence.instant().unix_millis(),
            definition_revision: occurrence.recurrence_revision(),
            occurrence_revision: claimed.revision(),
            latest_release: selection
                .latest_release()
                .map(RecurrenceOccurrenceRelease::as_str),
        }
    });
    let output = match serde_json::to_string(&ClaimNextRecurrenceOccurrenceInspection {
        occurrence,
        next_offset: selection.next_offset(),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_next_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
    cutoff_unix_millis: u64,
    raw_task_id: &str,
) -> ExitCode {
    const FAILURE_CODE: &str = "recurrence_occurrence_materialize_next_failed";
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let selection = match store.materialize_next_available_occurrence(
        &id,
        start_offset,
        page_size,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
        task_id,
    ) {
        Ok(selection) => selection,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let output = match serde_json::to_string(&MaterializeNextRecurrenceOccurrenceInspection {
        occurrence: selection
            .occurrence()
            .map(materialized_recurrence_occurrence_inspection),
        next_offset: selection.next_offset(),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn serialize_recurrence_occurrence_page(
    page: &RecurrenceOccurrencePage,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&RecurrenceOccurrencePageInspection {
        occurrences: page
            .occurrences()
            .iter()
            .map(recurrence_occurrence_inspection)
            .collect(),
        next_offset: page.next_offset(),
    })
}

fn select_latest_due_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_lookup_failed", error);
        }
    };
    let selection = match store.latest_due_occurrence(
        &id,
        start_offset,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(selection) => selection,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_lookup_failed", error);
        }
    };
    let output = match serialize_latest_due_occurrence_selection(&selection) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_lookup_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn persist_latest_due_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    start_offset: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_persistence_failed", error);
        }
    };
    let selection = match store.persist_latest_due_occurrence(
        &id,
        expected_revision,
        start_offset,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(selection) => selection,
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_persistence_failed", error);
        }
    };
    let output = match serialize_latest_due_occurrence_selection(&selection) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("latest_due_recurrence_occurrence_persistence_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn serialize_latest_due_occurrence_selection(
    selection: &vela_kernel::scheduler::LatestDueOccurrenceSelection,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&LatestDueOccurrenceInspection {
        occurrence: selection.occurrence().map(recurrence_occurrence_inspection),
        next_offset: selection.next_offset(),
    })
}

fn materialize_latest_due_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    start_offset: u64,
    cutoff_unix_millis: u64,
    raw_task_id: &str,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error(
                "latest_due_recurrence_occurrence_materialization_failed",
                error,
            );
        }
    };
    let selection = match store.materialize_latest_due_occurrence(
        &id,
        expected_revision,
        start_offset,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
        task_id,
    ) {
        Ok(selection) => selection,
        Err(error) => {
            return extension_error(
                "latest_due_recurrence_occurrence_materialization_failed",
                error,
            );
        }
    };
    let output = match serde_json::to_string(&LatestDueMaterializationInspection {
        occurrence: selection
            .occurrence()
            .map(materialized_recurrence_occurrence_inspection),
        next_offset: selection.next_offset(),
    }) {
        Ok(output) => output,
        Err(error) => {
            return extension_error(
                "latest_due_recurrence_occurrence_materialization_failed",
                error,
            );
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_materialized_recurrence_occurrences(
    database: &Path,
    raw_id: &str,
    start_offset: u64,
    raw_page_size: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let page_size = match OccurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_occurrence_page_size", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("materialized_recurrence_occurrence_lookup_failed", error);
        }
    };
    let page = match store.materialized_occurrences_page(&id, start_offset, page_size) {
        Ok(page) => page,
        Err(error @ RecurrenceStoreError::NotFound { .. }) => {
            return extension_error("recurrence_not_found", error);
        }
        Err(error @ RecurrenceStoreError::OccurrenceOutOfRange { .. }) => {
            return extension_error("recurrence_occurrence_out_of_range", error);
        }
        Err(error) => {
            return extension_error("materialized_recurrence_occurrence_lookup_failed", error);
        }
    };
    let output = match serialize_materialized_recurrence_occurrence_page(&page) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("materialized_recurrence_occurrence_lookup_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn serialize_materialized_recurrence_occurrence_page(
    page: &MaterializedRecurrenceOccurrencePage,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&MaterializedRecurrenceOccurrencePageInspection {
        occurrences: page
            .occurrences()
            .iter()
            .map(materialized_recurrence_occurrence_inspection)
            .collect(),
        next_offset: page.next_offset(),
    })
}

fn get_recurrence_occurrence(database: &Path, raw_id: &str, offset: u64) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_occurrence_lookup_failed", error),
    };
    let occurrence = match store.load_occurrence(&id, offset) {
        Ok(Some(occurrence)) => occurrence,
        Ok(None) => {
            return extension_error(
                "recurrence_occurrence_not_found",
                format!(
                    "recurrence occurrence ({:?}, {offset}) does not exist",
                    id.as_str()
                ),
            );
        }
        Err(error) => return extension_error("recurrence_occurrence_lookup_failed", error),
    };
    let output = match serde_json::to_string(&recurrence_occurrence_inspection(&occurrence)) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_occurrence_lookup_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn persist_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    expected_revision: u64,
    offset: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("recurrence_occurrence_persistence_failed", error);
        }
    };
    let occurrence = match store.persist_occurrence(&id, expected_revision, offset) {
        Ok(occurrence) => occurrence,
        Err(error) => {
            return extension_error("recurrence_occurrence_persistence_failed", error);
        }
    };
    let output = match serde_json::to_string(&recurrence_occurrence_inspection(&occurrence)) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("recurrence_occurrence_persistence_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn claim_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    offset: u64,
    expected_occurrence_revision: u64,
    cutoff_unix_millis: u64,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_occurrence_claim_failed", error),
    };
    let claimed = match store.claim_occurrence(
        &id,
        offset,
        expected_occurrence_revision,
        ScheduleInstant::from_unix_millis(cutoff_unix_millis),
    ) {
        Ok(claimed) => claimed,
        Err(error) => return extension_error("recurrence_occurrence_claim_failed", error),
    };
    let output = match serde_json::to_string(&claimed_recurrence_occurrence_inspection(&claimed)) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_occurrence_claim_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn release_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    offset: u64,
    expected_occurrence_revision: u64,
    raw_reason: &str,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let reason = match RecurrenceOccurrenceRelease::new(raw_reason) {
        Ok(reason) => reason,
        Err(error) => return extension_error("invalid_recurrence_occurrence_release", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_occurrence_release_failed", error),
    };
    let released = match store.release_occurrence(&id, offset, expected_occurrence_revision, reason)
    {
        Ok(released) => released,
        Err(error) => return extension_error("recurrence_occurrence_release_failed", error),
    };
    let occurrence = released.occurrence();
    let output = match serde_json::to_string(&ReleasedRecurrenceOccurrenceInspection {
        recurrence_id: occurrence.recurrence_id().as_str(),
        goal: occurrence.goal().as_str(),
        offset: occurrence.offset(),
        unix_millis: occurrence.instant().unix_millis(),
        definition_revision: occurrence.recurrence_revision(),
        occurrence_revision: released.revision(),
        latest_release: released.latest_release().as_str(),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_occurrence_release_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    offset: u64,
    expected_occurrence_revision: u64,
    raw_task_id: &str,
) -> ExitCode {
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => {
            return extension_error("recurrence_occurrence_materialization_failed", error);
        }
    };
    let materialized =
        match store.materialize_occurrence(&id, offset, expected_occurrence_revision, task_id) {
            Ok(materialized) => materialized,
            Err(error) => {
                return extension_error("recurrence_occurrence_materialization_failed", error);
            }
        };
    let output = match serde_json::to_string(&materialized_recurrence_occurrence_inspection(
        &materialized,
    )) {
        Ok(output) => output,
        Err(error) => {
            return extension_error("recurrence_occurrence_materialization_failed", error);
        }
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn materialize_claimed_recurrence_occurrence(
    database: &Path,
    raw_id: &str,
    offset: u64,
    expected_occurrence_revision: u64,
    raw_task_id: &str,
) -> ExitCode {
    const FAILURE_CODE: &str = "recurrence_claimed_occurrence_materialization_failed";
    let id = match RecurrenceId::new(raw_id) {
        Ok(id) => id,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let mut store = match RecurrenceStore::open(database) {
        Ok(store) => store,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let materialized = match store.materialize_claimed_occurrence(
        &id,
        offset,
        expected_occurrence_revision,
        task_id,
    ) {
        Ok(materialized) => materialized,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    let output = match serde_json::to_string(&materialized_recurrence_occurrence_inspection(
        &materialized,
    )) {
        Ok(output) => output,
        Err(error) => return extension_error(FAILURE_CODE, error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn load_recurrence(
    database: &Path,
    id: &RecurrenceId,
    failure_code: &'static str,
) -> Result<FixedIntervalRecurrence, ExitCode> {
    let store = RecurrenceStore::open_read_only(database)
        .map_err(|error| extension_error(failure_code, error))?;
    match store.load(id) {
        Ok(Some(recurrence)) => Ok(recurrence),
        Ok(None) => Err(extension_error(
            "recurrence_not_found",
            format!("recurrence {:?} does not exist", id.as_str()),
        )),
        Err(error) => Err(extension_error(failure_code, error)),
    }
}

fn recurrence_occurrence_inspection(
    occurrence: &RecurrenceOccurrence,
) -> RecurrenceOccurrenceInspection<'_> {
    RecurrenceOccurrenceInspection {
        recurrence_id: occurrence.recurrence_id().as_str(),
        goal: occurrence.goal().as_str(),
        offset: occurrence.offset(),
        unix_millis: occurrence.instant().unix_millis(),
        definition_revision: occurrence.recurrence_revision(),
    }
}

fn materialized_recurrence_occurrence_inspection(
    materialized: &MaterializedRecurrenceOccurrence,
) -> MaterializedRecurrenceOccurrenceInspection<'_> {
    let RecurrenceOccurrenceInspection {
        recurrence_id,
        goal,
        offset,
        unix_millis,
        definition_revision,
    } = recurrence_occurrence_inspection(materialized.occurrence());
    MaterializedRecurrenceOccurrenceInspection {
        recurrence_id,
        goal,
        offset,
        unix_millis,
        definition_revision,
        occurrence_revision: materialized.revision(),
        task_id: materialized.task_id().as_str(),
    }
}

fn inspect_recurrence_task(database: &Path, raw_task_id: &str) -> ExitCode {
    let task_id = match TaskId::new(raw_task_id) {
        Ok(task_id) => task_id,
        Err(error) => return extension_error("invalid_task_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_task_lookup_failed", error),
    };
    let materialized = match store.find_materialized_by_task_id(&task_id) {
        Ok(materialized) => materialized,
        Err(error) => return extension_error("recurrence_task_lookup_failed", error),
    };
    let output = match serde_json::to_string(&RecurrenceTaskInspection {
        task_id: task_id.as_str(),
        occurrence: materialized
            .as_ref()
            .map(materialized_recurrence_occurrence_inspection),
    }) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_task_lookup_failed", error),
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
    write_recurrence_inventory(&recurrences, "recurrence_inspection_failed")
}

fn page_recurrences(database: &Path, raw_page_size: u64, raw_after: Option<&str>) -> ExitCode {
    let page_size = match RecurrencePageSize::new(raw_page_size) {
        Ok(page_size) => page_size,
        Err(error) => return extension_error("invalid_recurrence_page_size", error),
    };
    let after = match raw_after.map(RecurrenceId::new).transpose() {
        Ok(after) => after,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_page_inspection_failed", error),
    };
    let page = match store.list_page(after.as_ref(), page_size) {
        Ok(page) => page,
        Err(error) => return extension_error("recurrence_page_inspection_failed", error),
    };
    let inventory = RecurrenceInventoryPage {
        recurrences: page
            .recurrences()
            .iter()
            .map(recurrence_inspection)
            .collect(),
        next_after: page.next_after().map(RecurrenceId::as_str),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_page_inspection_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn page_recurrences_by_status(
    database: &Path,
    raw_status: &str,
    raw_scan_size: u64,
    raw_after: Option<&str>,
) -> ExitCode {
    let status = match parse_recurrence_status(raw_status) {
        Ok(status) => status,
        Err(error) => return extension_error("invalid_recurrence_status", error),
    };
    let scan_size = match RecurrencePageSize::new(raw_scan_size) {
        Ok(scan_size) => scan_size,
        Err(error) => return extension_error("invalid_recurrence_page_size", error),
    };
    let after = match raw_after.map(RecurrenceId::new).transpose() {
        Ok(after) => after,
        Err(error) => return extension_error("invalid_recurrence_id", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_status_page_inspection_failed", error),
    };
    let page = match store.list_by_status_page(status, after.as_ref(), scan_size) {
        Ok(page) => page,
        Err(error) => return extension_error("recurrence_status_page_inspection_failed", error),
    };
    let inventory = RecurrenceInventoryPage {
        recurrences: page
            .recurrences()
            .iter()
            .map(recurrence_inspection)
            .collect(),
        next_after: page.next_after().map(RecurrenceId::as_str),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error("recurrence_status_page_inspection_failed", error),
    };
    println!("{output}");
    ExitCode::SUCCESS
}

fn inspect_recurrences_by_status(database: &Path, raw_status: &str) -> ExitCode {
    let status = match parse_recurrence_status(raw_status) {
        Ok(status) => status,
        Err(error) => return extension_error("invalid_recurrence_status", error),
    };
    let store = match RecurrenceStore::open_read_only(database) {
        Ok(store) => store,
        Err(error) => return extension_error("recurrence_status_inspection_failed", error),
    };
    let recurrences = match store.list_by_status(status) {
        Ok(recurrences) => recurrences,
        Err(error) => return extension_error("recurrence_status_inspection_failed", error),
    };
    write_recurrence_inventory(&recurrences, "recurrence_status_inspection_failed")
}

fn parse_recurrence_status(raw_status: &str) -> Result<RecurrenceStatus, &'static str> {
    match raw_status {
        "active" => Ok(RecurrenceStatus::Active),
        "cancelled" => Ok(RecurrenceStatus::Cancelled),
        _ => Err("expected active or cancelled"),
    }
}

fn write_recurrence_inventory(
    recurrences: &[FixedIntervalRecurrence],
    error_code: &str,
) -> ExitCode {
    let inventory = RecurrenceInventory {
        recurrences: recurrences.iter().map(recurrence_inspection).collect(),
    };
    let output = match serde_json::to_string(&inventory) {
        Ok(output) => output,
        Err(error) => return extension_error(error_code, error),
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
        status: match recurrence.status() {
            RecurrenceStatus::Active => "active",
            RecurrenceStatus::Cancelled => "cancelled",
        },
        final_occurrence_unix_millis: recurrence.final_occurrence().unix_millis(),
        definition_revision: recurrence.definition_revision(),
        aggregate_revision: recurrence.revision(),
        cancellation: recurrence.cancellation().map(|reason| reason.as_str()),
    }
}

fn parse_schedule_status(raw_status: &str) -> Result<ScheduleStatus, &'static str> {
    match raw_status {
        "pending" => Ok(ScheduleStatus::Pending),
        "cancelled" => Ok(ScheduleStatus::Cancelled),
        "claimed" => Ok(ScheduleStatus::Claimed),
        "materialized" => Ok(ScheduleStatus::Materialized),
        _ => Err("expected pending, cancelled, claimed, or materialized"),
    }
}

fn inspect_schedules_by_status(database: &Path, raw_status: &str) -> ExitCode {
    let status = match parse_schedule_status(raw_status) {
        Ok(status) => status,
        Err(error) => return extension_error("invalid_schedule_status", error),
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
