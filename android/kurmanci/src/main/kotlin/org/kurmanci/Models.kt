package org.kurmanci

data class PackInfo(
    val languageTag: String,
    val formatVersion: Int,
    val entryCount: Long
)

data class Candidate(
    val text: String,
    val kind: Int,
    val editCost: Int
)

data class SuggestionResult(
    val candidates: List<Candidate>
)

data class PredictionCandidate(
    val text: String,
    val count: Long,
    val probabilityMillionths: Int,
    val source: Int
)

data class PredictionResult(
    val candidates: List<PredictionCandidate>
)

data class EngineOptions(
    val maxCandidates: Int = 5
)
