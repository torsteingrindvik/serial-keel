#!/bin/bash

set -e

index=0
while [ $index -lt 1000 ]
do
echo "TEST RUN #$index"
index=`expr $index + 1`

cargo t --features mocks-share-endpoints --quiet
done
