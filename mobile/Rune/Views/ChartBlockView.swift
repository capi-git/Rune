// A chart block, as the Mac draws it (src/ui/charts.rs): a title, a short
// note, then bars, a line, a pie, a ring or the days of four weeks. What it
// draws comes from RuneCore (Chart.swift, ChartSource.swift).

import Charts
import RuneCore
import SwiftUI

struct ChartBlockView: View {
    let pageID: ID
    let block: Block

    @Environment(AppModel.self) private var model

    var body: some View {
        let chart = RuneCore.Chart.parse(block.data)
        let source = model.workspace.chartSource(page: pageID, block: block.id, from: chart.from)
        let today = model.when.today()
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(source.map { chart.heading($0.table) } ?? "Chart")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(Theme.text)
                if let source, source.elsewhere {
                    Text("from \(source.title)")
                        .font(.system(size: 12.5))
                        .foregroundStyle(Theme.text2)
                        .lineLimit(1)
                }
                Spacer(minLength: 4)
                if let source {
                    Text(chart.note(source.table, today: today))
                        .font(.system(size: 12.5, weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(Theme.text2)
                        .lineLimit(1)
                }
            }
            if let source {
                switch chart.shape {
                case .bars, .line:
                    PointsChart(chart: chart, table: source.table, points: chart.points(source.table, today: today))
                case .pie:
                    PieChart(chart: chart, table: source.table)
                case .ring:
                    RingChart(chart: chart, table: source.table)
                case .days:
                    DaysChart(days: chart.days(source.table, today: today))
                }
            } else {
                Text("No table to draw yet.")
                    .font(.system(size: 13))
                    .foregroundStyle(Theme.text2)
                    .frame(maxWidth: .infinity, minHeight: 80)
            }
        }
        .padding(14)
        .background(Theme.card, in: RoundedRectangle(cornerRadius: 16, style: .continuous))
        .overlay {
            RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(Theme.separator, lineWidth: 1)
        }
    }
}

private struct PointsChart: View {
    let chart: RuneCore.Chart
    let table: TableData
    let points: [ChartPoint]

    var body: some View {
        if points.isEmpty {
            Text("Nothing to draw yet.")
                .font(.system(size: 13))
                .foregroundStyle(Theme.text2)
                .frame(maxWidth: .infinity, minHeight: 80)
        } else {
            let signed = chart.signed(table)
            let items = Array(points.enumerated())
            let font = UIFont.systemFont(ofSize: 10.5)
            let widest = points.map { ($0.label as NSString).size(withAttributes: [.font: font]).width }.max() ?? 0
            Charts.Chart {
                if signed {
                    RuleMark(y: .value("Zero", 0))
                        .foregroundStyle(Theme.separator)
                }
                ForEach(items, id: \.offset) { i, point in
                    if chart.shape == .line {
                        LineMark(x: .value("When", "\(i)"), y: .value("Amount", point.value))
                            .foregroundStyle(Theme.ink)
                            .interpolationMethod(.monotone)
                        PointMark(x: .value("When", "\(i)"), y: .value("Amount", point.value))
                            .foregroundStyle(point.current ? Theme.now : Theme.ink)
                            .symbolSize(point.current ? 36 : 18)
                    } else {
                        BarMark(x: .value("When", "\(i)"), y: .value("Amount", point.value), width: .ratio(0.62))
                            .foregroundStyle(color(point, signed: signed))
                            .cornerRadius(3)
                    }
                }
            }
            .chartXAxis {
                AxisMarks(values: items.map { "\($0.offset)" }) { value in
                    AxisValueLabel(collisionResolution: .disabled) {
                        if let s = value.as(String.self), let i = Int(s), shows(i, points.count, widest: widest) {
                            Text(points[i].label)
                                .font(.system(size: 10.5))
                                .foregroundStyle(Theme.text2)
                                .fixedSize()
                        }
                    }
                }
            }
            .chartYAxis {
                AxisMarks(position: .leading, values: .automatic(desiredCount: 3)) { value in
                    AxisGridLine().foregroundStyle(Theme.separator)
                    AxisValueLabel {
                        if let v = value.as(Double.self) {
                            Text(chart.format(table, v))
                                .font(.system(size: 10.5))
                                .foregroundStyle(Theme.text2)
                        }
                    }
                }
            }
            .chartYScale(domain: .automatic(includesZero: true, reversed: chart.upsideDown(table)))
            .frame(height: 170)
            .accessibilityLabel(chart.heading(table))
        }
    }

    private func color(_ point: ChartPoint, signed: Bool) -> Color {
        if signed {
            return point.value < 0 ? TagLook.loss : TagLook.gain
        }
        return point.current ? Theme.ink : Theme.ink.opacity(0.45)
    }

    /// All the labels when they fit side by side, else every so many (as
    /// on the Mac), and always the last one. A phone is narrow: a label is
    /// left out sooner than on the Mac, so that none run into each other.
    private func shows(_ i: Int, _ n: Int, widest: CGFloat) -> Bool {
        let room: CGFloat = 270
        let fit = max(Int(room / (widest + 16)), 1)
        let every = max((n + fit - 1) / fit, 1)
        // The last label is always there; one just before it would touch it.
        if i + 1 == n { return true }
        return i % every == 0 && n - 1 - i >= every
    }
}

private struct PieChart: View {
    let chart: RuneCore.Chart
    let table: TableData

    var body: some View {
        let slices = chart.slices(table).filter { $0.value > 0 }
        if slices.isEmpty {
            Text("Nothing to draw yet.")
                .font(.system(size: 13))
                .foregroundStyle(Theme.text2)
                .frame(maxWidth: .infinity, minHeight: 80)
        } else {
            let all = slices.map(\.value).reduce(0, +)
            HStack(spacing: 18) {
                Charts.Chart(Array(slices.enumerated()), id: \.offset) { _, slice in
                    SectorMark(angle: .value("Amount", slice.value), innerRadius: .ratio(0.58), angularInset: 1.5)
                        .foregroundStyle(TagLook.dot(slice.color))
                        .cornerRadius(3)
                }
                .frame(width: 120, height: 120)
                VStack(alignment: .leading, spacing: 7) {
                    ForEach(Array(slices.enumerated()), id: \.offset) { _, slice in
                        HStack(spacing: 7) {
                            Circle().fill(TagLook.dot(slice.color)).frame(width: 8, height: 8)
                            Text(slice.label.isEmpty ? "None" : slice.label)
                                .font(.system(size: 13.5, weight: .medium))
                                .foregroundStyle(Theme.text)
                            Spacer(minLength: 4)
                            Text("\(chart.format(table, slice.value))  \(Int((slice.value / all * 100).rounded()))%")
                                .font(.system(size: 12.5))
                                .monospacedDigit()
                                .foregroundStyle(Theme.text2)
                        }
                    }
                }
            }
            .accessibilityElement(children: .combine)
        }
    }
}

private struct RingChart: View {
    let chart: RuneCore.Chart
    let table: TableData

    var body: some View {
        let value = chart.ring(table) ?? 0
        let goal = chart.goal ?? 0
        let part = goal > 0 ? min(max(value / goal, 0), 1) : 0
        HStack(spacing: 18) {
            ZStack {
                Circle().stroke(Theme.field, lineWidth: 10)
                Circle()
                    .trim(from: 0, to: part)
                    .stroke(Theme.ink, style: StrokeStyle(lineWidth: 10, lineCap: .round))
                    .rotationEffect(.degrees(-90))
                Text("\(Int((part * 100).rounded()))%")
                    .font(.system(size: 17, weight: .semibold))
                    .monospacedDigit()
                    .foregroundStyle(Theme.text)
            }
            .frame(width: 96, height: 96)
            VStack(alignment: .leading, spacing: 3) {
                Text(chart.format(table, value))
                    .font(.system(size: 20, weight: .semibold))
                    .monospacedDigit()
                    .foregroundStyle(Theme.text)
                if goal > 0 {
                    Text("of \(chart.format(table, goal))")
                        .font(.system(size: 13))
                        .foregroundStyle(Theme.text2)
                }
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// The last four weeks, Monday to Sunday: a filled square for a day with a row.
private struct DaysChart: View {
    let days: [ChartDay]

    var body: some View {
        let weeks = stride(from: 0, to: days.count, by: 7).map { Array(days[$0..<min($0 + 7, days.count)]) }
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 5) {
                ForEach(["M", "T", "W", "T", "F", "S", "S"].indices, id: \.self) { i in
                    Text(["M", "T", "W", "T", "F", "S", "S"][i])
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundStyle(Theme.text2)
                        .frame(maxWidth: .infinity)
                }
            }
            ForEach(weeks.indices, id: \.self) { w in
                HStack(spacing: 5) {
                    ForEach(weeks[w], id: \.date) { day in
                        RoundedRectangle(cornerRadius: 5, style: .continuous)
                            .fill(day.hasRow ? Theme.ink : Theme.field.opacity(day.ahead ? 0.4 : 1))
                            .overlay {
                                if day.today {
                                    RoundedRectangle(cornerRadius: 5, style: .continuous)
                                        .strokeBorder(Theme.now, lineWidth: 1.5)
                                }
                            }
                            .aspectRatio(1, contentMode: .fit)
                            .frame(maxWidth: .infinity)
                    }
                }
            }
        }
        .frame(maxWidth: 320)
        .accessibilityLabel("\(days.filter(\.hasRow).count) days with a row in four weeks")
    }
}
