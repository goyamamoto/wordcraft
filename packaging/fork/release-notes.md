WordCraft-ja は WordCraft の日本語強化版です(非公式のフォーク: https://github.com/goyamamoto/wordcraft )。

## 署名について

このリリースのアプリには開発元の署名・公証がありません(macOS は ad-hoc 署名、Windows は署名なし)。ダウンロードしたファイルを確かめたうえで、各自の判断で次の手順を行ってください。

### macOS (Apple Silicon / Intel 共通の universal 版)

1. `wordcraft-…-macos-universal.dmg` を開き、WordCraft を「アプリケーション」フォルダにドラッグします。
2. ターミナルで、ダウンロード時に付いた検疫情報を外します。

   ```sh
   xattr -dr com.apple.quarantine /Applications/WordCraft.app
   ```

### Windows (x64)

`.msi` か `.zip` を使います。起動時に「Windows によって PC が保護されました」と表示されたら、[詳細情報] → [実行] を選びます。

### Linux (x86_64)

AppImage、deb、rpm、tar.gz があります。AppImage は `chmod +x` してから実行します。

## 確認用

各ファイルの SHA-256 は `SHA256SUMS.txt` と下の一覧にあります(`shasum -a 256 <ファイル>` で照合できます)。
