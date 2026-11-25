<div align="center">

<img src="src-tauri/icons/icon.png" width="128" alt="pixiv-tool アイコン" />

# pixiv-tool

[![Version](https://img.shields.io/github/v/release/gbandszxc/pixiv-tool?label=version)](../../releases)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey)
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Vue](https://img.shields.io/badge/Vue-3-42B883?logo=vuedotjs&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-1.99.1-DEA584?logo=rust&logoColor=white)

[简体中文](README.md) | [English](README.en.md) | **日本語**

</div>

---

pixiv-tool は PC 向けのクロスプラットフォームなサードパーティ Pixiv クライアントです。イラスト・漫画・小説の閲覧と一括ダウンロードに加え、こだわりを持って設計した小説・漫画リーダーや、AI による小説まるごと翻訳といった機能を備えています。

## 開発の経緯

きっかけは、私が設計した小説翻訳ワークフロー用の skill を検証するために書いた、pixiv 小説の一括取得スクリプトでした。ターミナル操作が面倒なので AI に GUI に包ませ、でも閲覧とダウンロードが分断されたままで不満だったため、初期バージョンは pixiv の Web 版を webview で丸ごと埋め込んでいました。しかし見た目の違和感とリソース消費がどうにも納得できず、連休を使って普段使うページを独自 UI として一から作り直し、漫画と小説のリーダーも再設計しました。

せっかくここまでやったのだからと、バックエンドに Agent も組み込みました。これで翻訳と読書のワンストップ体験が完成し、最初に狙ったユースケースが一周回って閉じました……。

## ✨ 機能一覧

### 🌐 閲覧モード

pixiv の読み取り専用コンテンツを独自 UI で表示し、日々のブラウジングをまるごとカバーします。

- **ホームフィード**：おすすめと探索ギャラリーを混在表示。無限スクロール対応、イラスト／漫画／小説で絞り込み可能
- **チャンネルページ**：イラスト／漫画／小説の新作・おすすめ・ランキング・人気タグ。上部に R-18 クイックフィルター付き
- **フォローフィード**：フォロー中のユーザーの新着作品（イラスト／小説 × 全年齢／R-18）
- **検索**：キーワード検索と多軸ソート。数字だけの ID や Pixiv の URL から該当する作品・ユーザー・シリーズへ直接ジャンプ。Google 風の検索履歴サジェスト対応
- **ランキング／ブックマーク／閲覧履歴**：期間別ランキング。公開・非公開ブックマーク（タグ絞り込み対応）。ローカル閲覧履歴（ワンクリック消去、WebDAV 同期対応）
- **作品ビューア**：複数ページの逐次読み込み、全画面ページ送り＋フィルムストリップ式サムネイル、関連作品・コメントパネル、R-18 マスク
- **小説リーダー**：章送り・ページ送り・ルビ（ふりがな）対応、シリーズナビゲーション、関連作品とコメント表示
- **ソーシャル操作**：ブックマークの追加・解除、フォロー／解除、コメント・返信の投稿（テキスト・公式絵文字・スタンプ）
- **類似画像検索**：SauceNAO 連携。画像や URL をドラッグ＆ペーストするだけで出典作品を特定。検索履歴も保存されます

### 📥 ダウンロード

| 取得元 | 生成物 |
|---|---|
| 小説 1 本 / ユーザーの全小説 | TXT / Markdown / EPUB（EPUB は本文中の挿絵と表紙を埋め込み） |
| 小説シリーズ | まとめて取得：TXT / Markdown / EPUB として自動連結。Markdown と EPUB には目次付き |
| 単ページ・複数ページのイラスト、ユーザーの全イラスト・漫画 | 元画質のまま Pixiv の画像サーバーから直接ダウンロード |
| うごイラ | 元の解像度でループ再生する GIF |

### 📖 小説まるごと AI 翻訳

- **自分のモデルを使える**：OpenAI Chat Completions / Responses、Anthropic Messages の 3 プロトコルに対応。API キーは OS の資格情報ストアに保存されます
- **共有用語集**：翻訳前に作品全体で共有する固有名詞・別名・文体メモを整理できるので、章をまたいでも訳名がブレません
- **Agent カーネル駆動**：調整・翻訳・レビューのマルチロールが協調し、レビューは段階制——軽微な問題は配信をブロックせず、重大な問題はブロック単位でやり直します
- **完成された体験**：原文／訳文／対訳をいつでも切り替え可能。ストリーミングプレビューで思考の過程やトークン使用量を可視化。複数の小説をバックグラウンドで並列翻訳。確認待ちの状態は再起動後も復元。翻訳結果はファイルに書き出せます

### 🔐 アカウント・データ・アップデート

- **マルチアカウント**：ログインは独立した Chrome / Edge ウィンドウから Cookie を抽出する方式を優先し、未インストール時は内蔵ログインウィンドウへフォールバック。いつでも切り替え・ログアウト可能
- **資格情報の安全確保**：Cookie と API キーは OS の資格情報ストア（Windows Credential Manager / macOS Keychain / Linux Secret Service）にのみ保存。データベースにも入らず、UI やログに表示されることもありません
- **WebDAV 同期**：閲覧履歴・ログイン状態・SauceNAO キーを暗号化して自分のクラウドドライブへ同期。手動トリガー、エンドツーエンド暗号化
- **アプリ内アップデート**：更新の確認・ダウンロード・インストール案内まで完結。手動での再インストールは不要です

## 📦 インストール

[Releases](../../releases) から各プラットフォームのインストーラーをダウンロードしてください。

| プラットフォーム | インストーラー |
|---|---|
| Windows x64 / arm64 | `.msi` |
| macOS universal / aarch64 | `.dmg` |
| Linux x64 / arm64 | `.AppImage` / `.deb` / `.rpm` |

> Linux について：実行には `webkit2gtk` が必要です（Ubuntu/Debian：`sudo apt install libwebkit2gtk-4.1-dev`）。ログイン機能は現在 Windows / macOS で完全に動作します。Linux は実機での検証がまだです。

## 🔨 ソースからビルド

**前提条件**：Node.js 18 以上と [pnpm](https://pnpm.io/)。cmake（全プラットフォーム、BoringSSL のビルドに必要）。LLVM/libclang（Windows ビルドでは必須）。Rust のバージョンはリポジトリの `rust-toolchain.toml` で固定されており、`rustup` が自動で揃えてくれます。

```bash
# Windows（PowerShell）
.\dev.ps1 install      # フロントエンド依存をインストール
.\dev.ps1 dev start    # 開発サーバー起動：Vite + Rust ホットリロード + アプリウィンドウ

# macOS / Linux（Git Bash / Bash）
./dev.sh install
./dev.sh dev start
```

その他のよく使う操作：

```bash
.\dev.ps1 test         # Rust 単体テスト + IPC スモークテスト（完全オフライン）
.\dev.ps1 check        # フロントエンド型チェック + cargo check
.\dev.ps1 build        # リリース用インストーラーをパッケージ
.\dev.ps1 logs dev -f  # 開発ログを追跡
```

スクリプトの詳細は `.\dev.ps1 -h` の出力を参照してください。パッケージングの詳細は [docs/PACKAGING.md](docs/PACKAGING.md) をご覧ください。

## 🏗️ アーキテクチャ

Tauri 2 の単一プロセス構成：Rust バックエンドと Vue 3 / TypeScript フロントエンドが IPC で直接つながり、**ローカル HTTP サーバー・固定ポート・常駐バックエンドプロセスは一切ありません**。スクレイピングには wreq によるブラウザ TLS フィンガープリント偽装を使用し、小説翻訳は Rig 汎用 Agent カーネル上で動作します。

アーキテクチャの詳細、データモデル、IPC 契約は README では繰り返しません。真の情報源である [docs/SPEC.md](docs/SPEC.md) を直接ご覧ください。

## 📚 ドキュメント

> ※ 以下のドキュメントはすべて中国語で書かれています。

| ドキュメント | 内容 |
|---|---|
| [docs/SPEC.md](docs/SPEC.md) | プロジェクト仕様の唯一の情報源：範囲 / アーキテクチャ / データモデル / IPC / リスク登録 |
| [GLOSSARY.md](GLOSSARY.md) | ドメイン用語集 |
| [DESIGN.md](DESIGN.md) | フロントエンドのビジュアル・インタラクション仕様（Material 3 トークン体系） |
| [docs/PIXIV-API.md](docs/PIXIV-API.md) | Pixiv エンドポイント契約の事実源 |
| [docs/PACKAGING.md](docs/PACKAGING.md) | パッケージング / 配布 / ビルド環境 / 署名 |
| [docs/adr/](docs/adr/) | アーキテクチャ決定記録（ADR 0001 – 0055） |
| [docs/research/](docs/research/) | 外部 API 調査のエビデンスアーカイブ |
| [AGENTS.md](AGENTS.md) | AI コラボレーションの入口（agent が変更前に必読） |

## ⚠️ 免責事項

- 本ツールは、自分がアクセスする権利を持つコンテンツを個人的にローカルへ整理するためのものです。pixiv の利用規約とお住まいの地域の法令を遵守のうえご使用ください。一括クロール、再配布、商用目的での使用はできません。
- 本プロジェクトは pixiv（pixiv Inc.）と一切関係ありません。
- ローカルファースト・プライバシー重視の設計：エラーレポートやテレメトリは組み込んでいません。ログイン資格情報はお使いのマシンのシステム資格情報ストアにのみ保存されます。
