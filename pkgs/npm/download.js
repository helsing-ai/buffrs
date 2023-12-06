const fs = require('fs');
const https = require('https');
const os = require('os');
const pkg = require('./package.json');

const download = (url, dest) => {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https.get(url, (response) => {
      if (response.statusCode !== 200) {
        reject(new Error(`Failed to download: ${response.statusCode}`));
        return;
      }
      response.pipe(file);
    });

    file.on('finish', () => {
      file.close(resolve);
    });

    file.on('error', (err) => {
      fs.unlink(dest, () => reject(err));
    });
  });
};

const install = async () => {
  const arch = os.arch();
  const version = pkg.version;

  let platform = os.platform();
  let extension = "";

  if (["win32", "cygwin"].includes(process.platform)) {
    os = "windows";
    extension = ".exe";
  }

  const url = `https://github.com/helsing-ai/buffrs/releases/download/v${version}/buffrs-${platform}-${arch}${extension}`;
  const file = `./bin/buffrs-${platform}-${arch}${extension}`;

  try {
    await download(url, file);
    console.log('Buffrs downloaded successfully');
  } catch (e) {
    console.error('Failed to download binary:', e);
  }
};

install()
