FROM debian

RUN apt-get update && apt-get upgrade -y

RUN apt-get install -y \
      git \
      openssh-server

RUN sed -Ei "s/^PermitRootLogin.*/PermitRootLogin no/" /etc/ssh/sshd_config
RUN mkdir /var/run/sshd 
RUN adduser --system --shell /bin/bash --group --disabled-password --home /var/git/ git
RUN mkdir -p /var/git/.ssh

COPY git_server_keys /var/git/.ssh/authorized_keys
COPY git_server_keys /root/.ssh/authorized_keys
RUN mkdir /var/git/dummy-repo.git
RUN git init --bare /var/git/dummy-repo.git

RUN chown -R git:git /var/git
RUN chmod 700 /var/git/.ssh/authorized_keys

EXPOSE 22

# USER git
CMD /usr/sbin/sshd -D
