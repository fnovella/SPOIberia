use anyhow::Result;
use serde::Serialize;


use super::OutputFormat;

pub(super) fn print_execution_report(
    report: &crate::staging::ExecutionReport,
    format: OutputFormat,
) -> Result<()> {
    print_output(report, format, || {
        println!(
            "Estado: {} | Sitio: {} | Transacción: {:?}",
            report.status, report.site_url, report.transaction_id
        );
        println!("Resumen: {}", report.summary);
        println!("{}", "-".repeat(90));
        for op in &report.operations {
            println!("  {:>2}. [{:<8}] {}", op.seq_order, op.status, op.target);
            if let Some(ref err) = op.error {
                println!("      Error: {}", err);
            }
        }
    })
}

pub(super) fn print_output<T: Serialize, F: FnOnce()>(
    data: &T,
    format: OutputFormat,
    table_printer: F,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data)?);
        }
        OutputFormat::Csv => {
            println!("{}", serde_json::to_string(data)?);
        }
        OutputFormat::Table => {
            table_printer();
        }
    }
    Ok(())
}

pub(super) fn truncate(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

