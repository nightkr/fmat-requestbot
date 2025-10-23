select '- <@' || u.discord_user_id || '> - ' || count(*) || ' requests created'
  from request r
       inner join "user" u on r.created_by = u.id
 where r.created_at > :warstart
   and r.discord_guild_id = :guild
 group by u.discord_user_id
 order by count(*) desc;
