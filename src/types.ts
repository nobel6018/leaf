// Rust 쪽 직렬화 모양과 같다. 원본: crates/leaf-core, src-tauri/src/{library,search,export}.rs, transfer, inbox
export type DocumentKind = "pamphlet" | "book" | "document";
export type PageLayout = "single" | "spread";
export type PagePart = "whole" | "left" | "right";
export type Dewarp = "off" | "normal" | "strong";
export type Filter = "original" | "enhanced" | "grayscale" | "blackWhite";

export interface Point {
  x: number;
  y: number;
}

export interface Quad {
  topLeft: Point;
  topRight: Point;
  bottomRight: Point;
  bottomLeft: Point;
}

export type DocumentState =
  | { status: "unprocessed" }
  | { status: "stale"; pages: number }
  | { status: "ready" }
  | { status: "queued" }
  | { status: "processing"; done: number; total: number }
  | { status: "failed"; message: string };

export interface DocumentSummary {
  id: string;
  name: string;
  kind: DocumentKind;
  session: string | null;
  pageCount: number;
  createdAt: string;
  state: DocumentState;
  cover: string | null;
  hasSummary: boolean;
}

export interface ProcessOptions {
  dewarp: Dewarp;
  filter: Filter;
  removeFingers: boolean;
  removeHandwriting: boolean;
}

export interface Edits {
  name?: string;
  kind?: DocumentKind;
  session?: string;
  quads?: Record<string, Quad>;
  layouts?: Record<string, PageLayout>;
  deletedPages?: string[];
  pageOrder?: string[];
}

export interface OutputView {
  id: string;
  part: PagePart;
  image: string;
  thumb: string | null;
  hasOcr: boolean;
  processedAt: string;
}

export type QuadSource = "manual" | "phone" | "detected" | "none";

export interface PageView {
  id: string;
  raw: string;
  capturedAt: string;
  layout: PageLayout;
  quad: Quad | null;
  quadSource: QuadSource;
  outputs: OutputView[];
}

export interface DocumentDetail {
  summary: DocumentSummary;
  dir: string;
  pages: PageView[];
  deletedPages: string[];
  options: ProcessOptions;
  edits: Edits;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface OcrLine {
  text: string;
  bbox: Rect;
  confidence: number;
}

export interface OcrBlock {
  kind: "title" | "paragraph" | "list" | "table";
  text: string;
  bbox: Rect;
  lines: OcrLine[];
}

export interface OcrPage {
  engine: string;
  imageWidth: number;
  imageHeight: number;
  text: string;
  blocks: OcrBlock[];
}

export interface Segment {
  text: string;
  hit: boolean;
}

export interface PageHit {
  outputId: string;
  pageNumber: number;
  snippet: Segment[];
  matches: number;
}

export interface DocumentHits {
  documentId: string;
  name: string;
  session: string | null;
  kind: DocumentKind;
  nameMatch: boolean;
  hits: PageHit[];
  totalPages: number;
}

export type ExportFormat = "pdf" | "jpg" | "txt";

export interface ExportFile {
  path: string;
  name: string;
  size: number;
  modifiedAt: string;
}

export type Relevance = "competitor" | "partnerCandidate" | "supplier" | "unrelated";

export interface PamphletSummary {
  company: string;
  country: string;
  event: string;
  date: string;
  products: string;
  keySpecs: string;
  targetMarket: string;
  references: string;
  relevance: Relevance[];
  relevanceNote: string;
  contact: string;
  sourceFile: string;
  generatedBy: string;
  generatedAt: string;
  editedFields: string[];
}

export interface Progress {
  documentId: string;
  done: number;
  total: number;
  stage: "decode" | "correct" | "ocr" | "done";
}

export interface PairedDevice {
  deviceId: string;
  deviceName: string;
  pairedAt: string;
}

export interface TransferStatus {
  listening: boolean;
  port: number;
  macName: string;
  pairedDevices: PairedDevice[];
}

export interface PairingCode {
  code: string;
  expiresAt: string;
}

export interface InboxStatus {
  path: string;
  available: boolean;
  pending: number;
}
