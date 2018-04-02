readelf -aW $1 > /tmp/elfdiff1 
readelf -aW $2 > /tmp/elfdiff2
readelf -aW $3 > /tmp/elfdiff3
readelf -aW $4 > /tmp/elfdiff4
vimdiff /tmp/elfdiff1 /tmp/elfdiff2 /tmp/elfdiff3 /tmp/elfdiff4
