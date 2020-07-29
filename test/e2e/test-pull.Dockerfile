FROM debian

RUN apt-get update && apt-get install -y \
      git \
      openssh-client \
      curl

RUN mkdir /root/.ssh/
COPY upgit-linux /bin/upgit
COPY upgit_rsa /root/.ssh/id_rsa
RUN chmod 700 /root/.ssh/id_rsa
RUN echo 'StrictHostKeyChecking accept-new' >> /root/.ssh/config
COPY test-pull.sh test-pull.sh

CMD /bin/bash test-pull.sh
